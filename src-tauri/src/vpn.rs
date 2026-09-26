//! Жизненный цикл ядра mihomo: подготовка конфига, запуск, проверка
//! готовности, системный прокси, остановка и реакция на падение.

use crate::config::{self, Download, RuntimeOpts};
use crate::state::AppState;
use crate::util::{self, now_ms};
use crate::{cores, tray, winsys};
use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use sysproxy::Sysproxy;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::sync::oneshot;

const BYPASS: &str = "localhost;127.*;10.*;172.16.*;172.17.*;172.18.*;172.19.*;172.20.*;172.21.*;172.22.*;172.23.*;172.24.*;172.25.*;172.26.*;172.27.*;172.28.*;172.29.*;172.30.*;172.31.*;192.168.*;<local>";

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct VpnStatus {
    /// stopped | starting | running | stopping
    pub state: String,
    pub controller: Option<String>,
    pub secret: Option<String>,
    pub mixed_port: Option<u16>,
    pub started_at: Option<u64>,
    pub connection: Option<String>,
    pub profile_id: Option<String>,
    pub error: Option<String>,
    /// Что сейчас делается при запуске — для подписи под индикатором.
    pub stage: Option<String>,
}

struct Running {
    kill: oneshot::Sender<()>,
    done: oneshot::Receiver<()>,
}

pub struct VpnManager {
    status: Mutex<VpnStatus>,
    running: tokio::sync::Mutex<Option<Running>>,
    prev_proxy: Mutex<Option<Sysproxy>>,
    op: tokio::sync::Mutex<()>,
}

impl VpnManager {
    pub fn new() -> Self {
        Self {
            status: Mutex::new(VpnStatus { state: "stopped".into(), ..Default::default() }),
            running: tokio::sync::Mutex::new(None),
            prev_proxy: Mutex::new(None),
            op: tokio::sync::Mutex::new(()),
        }
    }

    pub fn status(&self) -> VpnStatus {
        self.status.lock().unwrap().clone()
    }

    pub fn mixed_port(&self) -> Option<u16> {
        let s = self.status.lock().unwrap();
        (s.state == "running").then_some(s.mixed_port).flatten()
    }

    pub fn controller(&self) -> Option<(String, String)> {
        let s = self.status.lock().unwrap();
        if s.state != "running" {
            return None;
        }
        Some((s.controller.clone()?, s.secret.clone()?))
    }
}

fn set_status(app: &AppHandle, f: impl FnOnce(&mut VpnStatus)) {
    let st = app.state::<AppState>();
    let snapshot = {
        let mut s = st.vpn.status.lock().unwrap();
        f(&mut s);
        s.clone()
    };
    let _ = app.emit("vpn-status", &snapshot);
    tray::refresh(app, &snapshot);
}

fn stage(app: &AppHandle, text: &str) {
    set_status(app, |s| s.stage = Some(text.into()));
}

fn log(app: &AppHandle, level: &str, msg: impl Into<String>) {
    app.state::<AppState>().logs.push(app, "app", level, msg);
}

pub async fn start(app: &AppHandle) -> Result<()> {
    let st = app.state::<AppState>();
    let _op = st.vpn.op.lock().await;
    if st.vpn.status().state == "running" {
        return Ok(());
    }
    set_status(app, |s| *s = VpnStatus { state: "starting".into(), ..Default::default() });
    match start_inner(app).await {
        Ok(()) => Ok(()),
        Err(e) => {
            let msg = format!("{e:#}");
            log(app, "error", format!("Не удалось подключиться: {msg}"));
            shutdown(app).await;
            set_status(app, |s| *s = VpnStatus { state: "stopped".into(), error: Some(msg), ..Default::default() });
            Err(e)
        }
    }
}

pub async fn stop(app: &AppHandle) -> Result<()> {
    let st = app.state::<AppState>();
    let _op = st.vpn.op.lock().await;
    if st.vpn.status().state == "stopped" {
        return Ok(());
    }
    set_status(app, |s| s.state = "stopping".into());
    shutdown(app).await;
    set_status(app, |s| *s = VpnStatus { state: "stopped".into(), ..Default::default() });
    log(app, "info", "VPN отключён");
    Ok(())
}

pub async fn restart(app: &AppHandle) -> Result<()> {
    stop(app).await?;
    start(app).await
}

/// Снимает системный прокси и останавливает процесс ядра.
async fn shutdown(app: &AppHandle) {
    restore_sysproxy(app);
    let st = app.state::<AppState>();
    let running = st.vpn.running.lock().await.take();
    if let Some(r) = running {
        let _ = r.kill.send(());
        let _ = tokio::time::timeout(Duration::from_secs(5), r.done).await;
    }
}

async fn start_inner(app: &AppHandle) -> Result<()> {
    let st = app.state::<AppState>();
    let settings = st.settings.lock().unwrap().clone();
    let profile_id = settings
        .active_profile
        .clone()
        .ok_or_else(|| anyhow!("Сначала добавьте подписку"))?;
    let tun = settings.connection == "tun";
    if tun && !winsys::is_elevated() {
        bail!("Для режима TUN нужны права администратора");
    }

    stage(app, "Подготовка ядра");
    let exe = cores::ensure(app, "mihomo").await?;

    let home = st.paths.mihomo_home.clone();
    let text = std::fs::read_to_string(st.paths.profile_content(&profile_id))
        .context("Файл профиля не найден — обновите подписку")?;
    let mut cfg: serde_yaml::Mapping = serde_yaml::from_str(&text).context("Ошибка в конфиге профиля")?;

    let mixed_port = if util::port_is_free(settings.mixed_port) { settings.mixed_port } else { util::free_port()? };
    let controller_port = util::free_port()?;
    let secret = util::random_hex(16);
    config::apply_overrides(
        &mut cfg,
        &RuntimeOpts {
            mixed_port,
            controller_port,
            secret: &secret,
            mode: &settings.mode,
            tun,
            app_rules: &settings.app_rules,
        },
    );

    // Пока ядро само грузит rule-provider'ы, оно не пропускает трафик. Поэтому
    // скачиваем недостающее заранее, напрямую, а не скачанное временно исключаем.
    stage(app, "Загрузка правил");
    let missing = config::missing_providers(&mut cfg, &home);
    let geo = config::missing_geo(&cfg, &home);
    let failed = prefetch(app, missing.into_iter().chain(geo).collect()).await;
    let full = cfg.clone();
    let excluded: Vec<Download> = failed.into_iter().filter(|d| d.rule).collect();
    let excluded_names: Vec<String> = excluded.iter().map(|d| d.name.clone()).collect();
    config::exclude_rule_providers(&mut cfg, &excluded_names);
    if !excluded.is_empty() {
        log(app, "warning", format!("Временно без правил: {} — догружу после подключения", excluded_names.join(", ")));
    }
    let config_path = home.join("config.yaml");
    util::write_atomic(&config_path, serde_yaml::to_string(&cfg)?)?;

    stage(app, "Запуск ядра");
    let exited = Arc::new(AtomicBool::new(false));
    spawn_core(app, &exe, &home, &config_path, exited.clone()).await?;

    let t0 = Instant::now();
    wait_ready(mixed_port, controller_port, &secret, &exited).await?;
    log(app, "info", format!("Ядро готово за {} мс", t0.elapsed().as_millis()));

    if !tun {
        enable_sysproxy(app, mixed_port)?;
    }
    set_status(app, |s| {
        *s = VpnStatus {
            state: "running".into(),
            controller: Some(format!("127.0.0.1:{controller_port}")),
            secret: Some(secret.clone()),
            mixed_port: Some(mixed_port),
            started_at: Some(now_ms()),
            connection: Some(settings.connection.clone()),
            profile_id: Some(profile_id.clone()),
            error: None,
            stage: None,
        }
    });

    if !excluded.is_empty() {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = late_providers(&app, excluded, full, mixed_port, controller_port, secret, config_path).await {
                log(&app, "warning", format!("Не удалось догрузить правила: {e:#}"));
            }
        });
    }
    Ok(())
}

/// Параллельно скачивает файлы напрямую; возвращает то, что скачать не удалось.
async fn prefetch(app: &AppHandle, items: Vec<Download>) -> Vec<Download> {
    if items.is_empty() {
        return vec![];
    }
    let Ok(client) = util::direct_client(Duration::from_secs(15)) else { return items };
    let results = futures::future::join_all(items.into_iter().map(|d| {
        let client = client.clone();
        async move {
            let r = util::download_to(&client, &d.url, &d.path).await;
            (d, r)
        }
    }))
    .await;
    let mut failed = Vec::new();
    let mut ok = 0;
    for (d, r) in results {
        match r {
            Ok(()) => ok += 1,
            Err(e) => {
                log(app, "warning", format!("Не скачан {}: {e:#}", d.name));
                failed.push(d);
            }
        }
    }
    if ok > 0 {
        log(app, "info", format!("Скачано файлов правил и баз: {ok}"));
    }
    failed
}

/// После подключения догружает исключённые правила через VPN и перечитывает
/// конфиг через API — без переподключения.
async fn late_providers(
    app: &AppHandle,
    pending: Vec<Download>,
    mut full: serde_yaml::Mapping,
    mixed_port: u16,
    controller_port: u16,
    secret: String,
    config_path: PathBuf,
) -> Result<()> {
    let client = util::proxied_client(mixed_port, Duration::from_secs(30))?;
    let mut still_failed = Vec::new();
    for d in &pending {
        if let Err(e) = util::download_to(&client, &d.url, &d.path).await {
            log(app, "warning", format!("{} не скачан и через VPN: {e:#}", d.name));
            still_failed.push(d.name.clone());
        }
    }
    if still_failed.len() == pending.len() {
        return Ok(());
    }
    config::exclude_rule_providers(&mut full, &still_failed);
    // Пока качали, VPN могли отключить или переподключить — тогда не трогаем.
    let st = app.state::<AppState>();
    if st.vpn.controller().map(|(c, _)| c) != Some(format!("127.0.0.1:{controller_port}")) {
        return Ok(());
    }
    util::write_atomic(&config_path, serde_yaml::to_string(&full)?)?;
    let ctl = util::direct_client(Duration::from_secs(20))?;
    ctl.put(format!("http://127.0.0.1:{controller_port}/configs?force=true"))
        .bearer_auth(&secret)
        .json(&serde_json::json!({ "path": config_path.to_string_lossy() }))
        .send()
        .await?
        .error_for_status()?;
    log(app, "info", "Правила догружены, конфиг перечитан");
    Ok(())
}

async fn spawn_core(
    app: &AppHandle,
    exe: &std::path::Path,
    home: &std::path::Path,
    config_path: &std::path::Path,
    exited: Arc<AtomicBool>,
) -> Result<()> {
    let mut cmd = tokio::process::Command::new(exe);
    cmd.arg("-d").arg(home).arg("-f").arg(config_path);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.creation_flags(winsys::CREATE_NO_WINDOW);
    cmd.kill_on_drop(true);
    let mut child = cmd.spawn().context("не удалось запустить mihomo")?;
    if let Some(h) = child.raw_handle() {
        winsys::attach_to_job(h);
    }
    if let Some(out) = child.stdout.take() {
        pipe_logs(app.clone(), out);
    }
    if let Some(err) = child.stderr.take() {
        pipe_logs(app.clone(), err);
    }

    let (kill_tx, kill_rx) = oneshot::channel::<()>();
    let (done_tx, done_rx) = oneshot::channel::<()>();
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let unexpected = tokio::select! {
            status = child.wait() => Some(status.ok().and_then(|s| s.code())),
            _ = kill_rx => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                None
            }
        };
        exited.store(true, Ordering::SeqCst);
        let _ = done_tx.send(());
        if let Some(code) = unexpected {
            on_crash(&app2, code).await;
        }
    });
    *app.state::<AppState>().vpn.running.lock().await = Some(Running { kill: kill_tx, done: done_rx });
    Ok(())
}

async fn on_crash(app: &AppHandle, code: Option<i32>) {
    let st = app.state::<AppState>();
    if st.vpn.status().state != "running" {
        return; // при запуске ошибку покажет start()
    }
    st.vpn.running.lock().await.take();
    restore_sysproxy(app);
    let msg = format!("Ядро неожиданно завершилось (код {})", code.map_or("?".into(), |c| c.to_string()));
    log(app, "error", &msg);
    set_status(app, |s| *s = VpnStatus { state: "stopped".into(), error: Some(msg), ..Default::default() });
}

fn pipe_logs(app: AppHandle, stream: impl AsyncRead + Unpin + Send + 'static) {
    tauri::async_runtime::spawn(async move {
        let mut reader = BufReader::new(stream);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let line = String::from_utf8_lossy(&buf);
                    let line = line.trim_end();
                    if line.is_empty() {
                        continue;
                    }
                    let (level, msg) = crate::logs::parse_mihomo_line(line);
                    app.state::<AppState>().logs.push(&app, "vpn", &level, msg);
                }
            }
        }
    });
}

/// Ядро готово, когда через его же SOCKS-порт отвечает его контроллер.
/// Приостановленное ядро рвёт соединение сразу, так что это не путается
/// с «сервер VPN не отвечает».
async fn wait_ready(mixed_port: u16, controller_port: u16, secret: &str, exited: &AtomicBool) -> Result<()> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .proxy(reqwest::Proxy::all(format!("socks5h://127.0.0.1:{mixed_port}"))?)
        .timeout(Duration::from_millis(1500))
        .build()?;
    let url = format!("http://127.0.0.1:{controller_port}/version");
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if exited.load(Ordering::SeqCst) {
            bail!("ядро завершилось при запуске — подробности в журнале");
        }
        if let Ok(r) = client.get(&url).bearer_auth(secret).send().await {
            if r.status().is_success() {
                return Ok(());
            }
        }
        if Instant::now() > deadline {
            bail!("ядро не ответило за 60 секунд");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

// ---------- системный прокси ----------

fn points_to(p: &Sysproxy, port: Option<u16>) -> bool {
    p.enable && port.is_some_and(|port| p.port == port) && (p.host == "127.0.0.1" || p.host == "localhost")
}

fn enable_sysproxy(app: &AppHandle, port: u16) -> Result<()> {
    let st = app.state::<AppState>();
    let ours = st.settings.lock().unwrap().sysproxy_port;
    let prev = Sysproxy::get_system_proxy().ok().filter(|p| !points_to(p, ours));
    *st.vpn.prev_proxy.lock().unwrap() = prev;
    Sysproxy { enable: true, host: "127.0.0.1".into(), port, bypass: BYPASS.into() }
        .set_system_proxy()
        .map_err(|e| anyhow!("не удалось включить системный прокси: {e}"))?;
    st.update_settings(|s| s.sysproxy_port = Some(port))?;
    Ok(())
}

/// Возвращает прокси Windows как было до подключения. Вызывается и при
/// старте приложения — на случай, если прошлый запуск завершился аварийно.
pub fn restore_sysproxy(app: &AppHandle) {
    let st = app.state::<AppState>();
    let ours = st.settings.lock().unwrap().sysproxy_port;
    if ours.is_none() {
        return;
    }
    let prev = st.vpn.prev_proxy.lock().unwrap().take();
    let result = match prev {
        Some(p) if p.enable => p.set_system_proxy(),
        _ => match Sysproxy::get_system_proxy() {
            Ok(mut cur) if points_to(&cur, ours) => {
                cur.enable = false;
                cur.set_system_proxy()
            }
            _ => Ok(()),
        },
    };
    if let Err(e) = result {
        log(app, "warning", format!("Не удалось вернуть системный прокси: {e}"));
    }
    let _ = st.update_settings(|s| s.sysproxy_port = None);
}

/// Меняет режим (правила/глобальный/прямой) на лету через API контроллера.
pub async fn patch_mode(app: &AppHandle, mode: &str) -> Result<()> {
    let Some((ctl, secret)) = app.state::<AppState>().vpn.controller() else { return Ok(()) };
    util::direct_client(Duration::from_secs(5))?
        .patch(format!("http://{ctl}/configs"))
        .bearer_auth(secret)
        .json(&serde_json::json!({ "mode": mode }))
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}
