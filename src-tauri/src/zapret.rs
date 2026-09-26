//! Обход DPI без VPN: zapret (winws.exe + WinDivert).
//!
//! winws перехватывает исходящие пакеты драйвером WinDivert и «портит» их так,
//! что DPI провайдера не может разобрать запрос, а сервер — понимает. Поэтому
//! нужны права администратора: драйвер ставится в систему.

use crate::child::{self, Running, Spawn};
use crate::state::AppState;
use crate::util::now_ms;
use crate::{cores, winsys};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::ffi::OsString;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ZapretStatus {
    /// stopped | starting | running | stopping
    pub state: String,
    pub started_at: Option<u64>,
    pub error: Option<String>,
    /// Название применённой стратегии.
    pub strategy: Option<String>,
}

pub struct ZapretManager {
    status: Mutex<ZapretStatus>,
    running: tokio::sync::Mutex<Option<Running>>,
    op: tokio::sync::Mutex<()>,
}

impl ZapretManager {
    pub fn new() -> Self {
        Self {
            status: Mutex::new(ZapretStatus { state: "stopped".into(), ..Default::default() }),
            running: tokio::sync::Mutex::new(None),
            op: tokio::sync::Mutex::new(()),
        }
    }

    pub fn status(&self) -> ZapretStatus {
        self.status.lock().unwrap().clone()
    }
}

fn set_status(app: &AppHandle, f: impl FnOnce(&mut ZapretStatus)) {
    let st = app.state::<AppState>();
    let snapshot = {
        let mut s = st.zapret.status.lock().unwrap();
        f(&mut s);
        s.clone()
    };
    let _ = app.emit("zapret-status", &snapshot);
}

fn log(app: &AppHandle, level: &str, msg: impl Into<String>) {
    app.state::<AppState>().logs.push(app, "dpi", level, msg);
}

pub struct Strategy {
    pub id: &'static str,
    pub title: &'static str,
    pub about: &'static str,
    /// Аргументы winws. `@fake/<файл>` подставляется путём к заготовке пакета.
    args: &'static [&'static str],
}

/// Стратегии из набора zapret для России: первая подходит большинству.
pub const STRATEGIES: &[Strategy] = &[
    Strategy {
        id: "general",
        title: "Основная",
        about: "TCP 80/443 и QUIC. Подходит большинству провайдеров",
        args: &[
            "--wf-tcp=80,443",
            "--wf-udp=443,50000-50100",
            "--filter-udp=443",
            "--dpi-desync=fake",
            "--dpi-desync-repeats=6",
            "--dpi-desync-fake-quic=@fake/quic_initial_www_google_com.bin",
            "--new",
            "--filter-udp=50000-50100",
            "--filter-l7=discord,stun",
            "--dpi-desync=fake",
            "--dpi-desync-repeats=6",
            "--new",
            "--filter-tcp=80",
            "--dpi-desync=fake,split2",
            "--dpi-desync-autottl=2",
            "--dpi-desync-fooling=md5sig",
            "--new",
            "--filter-tcp=443",
            "--dpi-desync=fake,split",
            "--dpi-desync-autottl=2",
            "--dpi-desync-fooling=badseq",
            "--dpi-desync-fake-tls=@fake/tls_clienthello_www_google_com.bin",
        ],
    },
    Strategy {
        id: "split",
        title: "Дробление",
        about: "Без поддельных пакетов — когда основная не помогает",
        args: &[
            "--wf-tcp=80,443",
            "--wf-udp=443",
            "--filter-udp=443",
            "--dpi-desync=fake",
            "--dpi-desync-repeats=8",
            "--dpi-desync-fake-quic=@fake/quic_initial_www_google_com.bin",
            "--new",
            "--filter-tcp=80,443",
            "--dpi-desync=split2",
            "--dpi-desync-split-pos=1",
            "--dpi-desync-split-seqovl=1",
        ],
    },
];

pub fn strategy(id: &str) -> &'static Strategy {
    STRATEGIES.iter().find(|s| s.id == id).unwrap_or(&STRATEGIES[0])
}

/// Аргументы для запуска: подставляет пути к заготовкам пакетов и, если включён
/// список доменов, ограничивает им каждый профиль (кроме UDP-профиля Discord,
/// где имени сайта в пакете нет).
fn build_args(args: &[String], dir: &Path, hostlist: Option<&Path>) -> Vec<OsString> {
    let expand = |a: &str| -> OsString {
        match a.split_once("=@") {
            Some((key, rel)) => {
                let mut v = OsString::from(key);
                v.push("=");
                v.push(dir.join(rel));
                v
            }
            None => OsString::from(a),
        }
    };
    let mut out: Vec<OsString> = Vec::new();
    for profile in args.split(|a| a == "--new") {
        if !out.is_empty() {
            out.push(OsString::from("--new"));
        }
        for a in profile {
            out.push(expand(a));
        }
        let is_discord = profile.iter().any(|a| a.contains("discord"));
        if let Some(path) = hostlist.filter(|_| !is_discord && !profile.is_empty()) {
            let mut v = OsString::from("--hostlist=");
            v.push(path);
            out.push(v);
        }
    }
    out
}

/// Набор аргументов выбранной стратегии: встроенной или своей.
fn strategy_args(set: &crate::settings::Settings) -> Result<(String, Vec<String>)> {
    if set.zapret_strategy == CUSTOM {
        let raw = set.zapret_custom.trim();
        if raw.is_empty() {
            bail!("Своя стратегия пустая — впишите аргументы winws или выберите готовую");
        }
        return Ok(("Своя".to_string(), split_args(raw)));
    }
    let s = strategy(&set.zapret_strategy);
    Ok((s.title.to_string(), s.args.iter().map(|a| a.to_string()).collect()))
}

/// Идентификатор своей стратегии.
pub const CUSTOM: &str = "custom";

/// Разбивает строку аргументов по пробелам и переводам строк, уважая кавычки.
pub fn split_args(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for c in raw.chars() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => cur.push(c),
            (None, '"') | (None, '\'') => quote = Some(c),
            (None, _) if c.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            (None, _) => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Файл со списком доменов. Лежит рядом с настройками, а не в папке ядра:
/// папка ядра при обновлении заменяется целиком.
fn hostlist_path(app: &AppHandle) -> std::path::PathBuf {
    app.state::<AppState>().paths.root.join("zapret-hostlist.txt")
}

/// Путь к списку доменов, если он включён и не пуст.
fn active_hostlist(app: &AppHandle) -> Option<std::path::PathBuf> {
    let st = app.state::<AppState>();
    if !st.settings.lock().unwrap().zapret_hostlist_on {
        return None;
    }
    let p = hostlist_path(app);
    std::fs::metadata(&p).ok().filter(|m| m.len() > 0).map(|_| p)
}

/// Пересобирает файл списка из отмеченных наборов и своих адресов.
/// Возвращает число записей.
pub fn rebuild_hostlist(app: &AppHandle) -> Result<usize> {
    let st = app.state::<AppState>();
    let (sets, custom) = {
        let s = st.settings.lock().unwrap();
        (s.zapret_sets.clone(), s.zapret_custom_hosts.clone())
    };
    // По одному адресу в строке — формат zapret; поддомены подхватываются сами.
    let hosts = crate::hostsets::collect(&sets, &custom);
    crate::util::write_atomic(&hostlist_path(app), hosts.join("\r\n"))?;
    Ok(hosts.len())
}

pub async fn start(app: &AppHandle) -> Result<()> {
    let st = app.state::<AppState>();
    let _op = st.zapret.op.lock().await;
    if st.zapret.status().state == "running" {
        return Ok(());
    }
    set_status(app, |s| *s = ZapretStatus { state: "starting".into(), ..Default::default() });
    match start_inner(app).await {
        Ok(()) => Ok(()),
        Err(e) => {
            let msg = format!("{e:#}");
            log(app, "error", format!("Не удалось включить обход: {msg}"));
            if let Some(r) = st.zapret.running.lock().await.take() {
                r.stop().await;
            }
            set_status(app, |s| *s = ZapretStatus { state: "stopped".into(), error: Some(msg), ..Default::default() });
            Err(e)
        }
    }
}

async fn start_inner(app: &AppHandle) -> Result<()> {
    // WinDivert ставит драйвер в систему — без прав администратора никак.
    if !winsys::is_elevated() {
        bail!("Нужны права администратора: обход работает через драйвер WinDivert");
    }
    // Два обхода DPI одновременно перехватывают одни и те же пакеты через
    // WinDivert и рвут соединения — проверено: интернет пропадает целиком.
    if let Some((pid, owner)) = winsys::find_process("winws.exe") {
        bail!("winws.exe уже запущен другой программой ({owner}, PID {pid}). Два обхода блокировок вместе ломают сеть — закройте ту программу");
    }
    let st = app.state::<AppState>();
    let exe = cores::ensure(app, "zapret").await?;
    let dir = exe.parent().context("нет папки ядра")?.to_path_buf();
    let (title, raw_args) = {
        let set = st.settings.lock().unwrap();
        strategy_args(&set)?
    };
    let args = build_args(&raw_args, &dir, active_hostlist(app).as_deref());

    let exited = Arc::new(AtomicBool::new(false));
    let running = child::spawn(
        app,
        Spawn {
            exe: &exe,
            args: args.iter().map(|a| a.as_os_str()).collect(),
            cwd: Some(&dir),
            source: "dpi",
            parse: parse_line,
            exited: exited.clone(),
        },
        |app, code| async move { on_crash(&app, code).await },
    )
    .await?;
    *st.zapret.running.lock().await = Some(running);

    // Своего сигнала готовности у winws нет: если параметры или драйвер
    // не подошли, он выходит в первые же мгновения. Даём ему это время.
    tokio::time::sleep(Duration::from_millis(1200)).await;
    if exited.load(Ordering::SeqCst) {
        bail!("ядро завершилось сразу после запуска — подробности в журнале");
    }
    set_status(app, |st| {
        *st = ZapretStatus {
            state: "running".into(),
            started_at: Some(now_ms()),
            error: None,
            strategy: Some(title.clone()),
        }
    });
    log(app, "info", format!("Обход блокировок включён, стратегия «{title}»"));
    Ok(())
}

pub async fn stop(app: &AppHandle) -> Result<()> {
    let st = app.state::<AppState>();
    let _op = st.zapret.op.lock().await;
    if st.zapret.status().state == "stopped" {
        return Ok(());
    }
    set_status(app, |s| s.state = "stopping".into());
    if let Some(r) = st.zapret.running.lock().await.take() {
        r.stop().await;
    }
    set_status(app, |s| *s = ZapretStatus { state: "stopped".into(), ..Default::default() });
    log(app, "info", "Обход блокировок выключен");
    Ok(())
}

async fn on_crash(app: &AppHandle, code: Option<i32>) {
    let st = app.state::<AppState>();
    if st.zapret.status().state != "running" {
        return;
    }
    st.zapret.running.lock().await.take();
    let msg = format!("Обход неожиданно завершился (код {})", code.map_or("?".into(), |c| c.to_string()));
    log(app, "error", &msg);
    set_status(app, |s| *s = ZapretStatus { state: "stopped".into(), error: Some(msg), ..Default::default() });
}

/// Проверяет набор аргументов без запуска: winws разбирает их и выходит.
pub async fn check(app: &AppHandle, raw_args: &[String]) -> Result<()> {
    let exe = cores::ensure(app, "zapret").await?;
    let dir = exe.parent().context("нет папки ядра")?.to_path_buf();
    let mut args = build_args(raw_args, &dir, active_hostlist(app).as_deref());
    args.insert(0, OsString::from("--dry-run"));
    let out = tokio::process::Command::new(&exe)
        .args(&args)
        .current_dir(&dir)
        .creation_flags(winsys::CREATE_NO_WINDOW)
        .output()
        .await?;
    if !out.status.success() {
        let text = String::from_utf8_lossy(&out.stderr);
        let line = text
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("ошибка в параметрах стратегии");
        bail!("{}", line.trim());
    }
    Ok(())
}

/// Аргументы стратегии по её идентификатору — для проверки перед сохранением.
pub fn args_of(app: &AppHandle, id: &str) -> Result<Vec<String>> {
    let st = app.state::<AppState>();
    let mut set = st.settings.lock().unwrap().clone();
    set.zapret_strategy = id.to_string();
    Ok(strategy_args(&set)?.1)
}

/// winws пишет простым текстом, уровень отмечает словами в начале строки.
fn parse_line(line: &str) -> (String, String) {
    let l = line.trim();
    let low = l.to_ascii_lowercase();
    let level = if low.starts_with("error") || low.contains("failed") || low.contains("cannot") {
        "error"
    } else if low.starts_with("warning") || low.starts_with("warn") {
        "warning"
    } else {
        "info"
    };
    (level.into(), l.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owned(id: &str) -> Vec<String> {
        strategy(id).args.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn fake_files_get_absolute_paths() {
        let args = build_args(&owned("general"), Path::new("C:\\cores\\zapret"), None);
        let tls = args.iter().find(|a| a.to_string_lossy().starts_with("--dpi-desync-fake-tls=")).unwrap();
        assert_eq!(
            tls.to_string_lossy(),
            "--dpi-desync-fake-tls=C:\\cores\\zapret\\fake/tls_clienthello_www_google_com.bin"
        );
        // Обычные параметры не трогаем.
        assert!(args.iter().any(|a| a == "--dpi-desync-autottl=2"));
    }

    #[test]
    fn hostlist_skips_discord_profile() {
        let list = Path::new(r"C:\data\hostlist.txt");
        let args = build_args(&owned("general"), Path::new(r"C:\cores\zapret"), Some(list));
        let joined: Vec<String> = args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
        // По профилю на каждый --new: их четыре, но у discord списка быть не должно.
        assert_eq!(joined.iter().filter(|a| a.starts_with("--hostlist=")).count(), 3);
        let discord = joined.iter().position(|a| a.contains("discord")).unwrap();
        let next_new = joined[discord..].iter().position(|a| a == "--new").unwrap() + discord;
        assert!(!joined[discord..next_new].iter().any(|a| a.starts_with("--hostlist=")));
    }

    #[test]
    fn splits_quoted_args() {
        assert_eq!(
            split_args("--wf-tcp=80,443  --dpi-desync=fake
--comment=\"два слова\""),
            vec!["--wf-tcp=80,443", "--dpi-desync=fake", "--comment=два слова"]
        );
    }

    #[test]
    fn unknown_strategy_falls_back() {
        assert_eq!(strategy("нет такой").id, "general");
    }
}

// ---------- автоподбор ----------

/// Домены, на которых проверяем обход, если пользователь не задал свои.
const DEFAULT_PROBES: &[&str] = &["discord.com", "www.youtube.com", "rutracker.org", "x.com"];

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub id: String,
    pub title: String,
    pub ok: usize,
    pub total: usize,
    pub failed: Vec<String>,
    pub error: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Progress {
    step: usize,
    total: usize,
    title: String,
}

/// Проверяет один домен: важно именно установление TLS-соединения — DPI рвёт
/// его на приветствии, а не отдаёт ошибку HTTP.
async fn probe(domain: &str) -> bool {
    let Ok(client) = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(6))
        .danger_accept_invalid_certs(true)
        .build()
    else {
        return false;
    };
    client.head(format!("https://{domain}/")).send().await.is_ok()
}

async fn probe_all(domains: &[String]) -> (usize, Vec<String>) {
    let results = futures::future::join_all(domains.iter().map(|d| async move { (d.clone(), probe(d).await) })).await;
    let failed: Vec<String> = results.iter().filter(|(_, ok)| !ok).map(|(d, _)| d.clone()).collect();
    (results.len() - failed.len(), failed)
}

/// Перебирает стратегии и проверяет по ним доступность доменов.
/// Возвращает сравнение, включая замер без обхода.
pub async fn autotune(app: &AppHandle, domains: Vec<String>) -> Result<Vec<ProbeResult>> {
    if !winsys::is_elevated() {
        bail!("Нужны права администратора: подбор запускает обход");
    }
    if let Some((pid, owner)) = winsys::find_process("winws.exe") {
        let st = app.state::<AppState>();
        if st.zapret.status().state != "running" {
            bail!("winws.exe уже запущен другой программой ({owner}, PID {pid}) — закройте её");
        }
    }
    let domains: Vec<String> = if domains.is_empty() {
        DEFAULT_PROBES.iter().map(|d| d.to_string()).collect()
    } else {
        domains
    };

    let st = app.state::<AppState>();
    let was_running = st.zapret.status().state == "running";
    if was_running {
        stop(app).await?;
    }
    let exe = cores::ensure(app, "zapret").await?;
    let dir = exe.parent().context("нет папки ядра")?.to_path_buf();
    let hostlist = active_hostlist(app);

    // Кандидаты: сначала «как есть», затем встроенные, затем своя.
    let mut candidates: Vec<(String, String, Option<Vec<String>>)> =
        vec![("none".into(), "Без обхода".into(), None)];
    for s in STRATEGIES {
        candidates.push((s.id.into(), s.title.into(), Some(s.args.iter().map(|a| a.to_string()).collect())));
    }
    let custom = st.settings.lock().unwrap().zapret_custom.trim().to_string();
    if !custom.is_empty() {
        candidates.push((CUSTOM.into(), "Своя".into(), Some(split_args(&custom))));
    }

    let total = candidates.len();
    let mut out = Vec::new();
    for (i, (id, title, args)) in candidates.into_iter().enumerate() {
        let _ = app.emit("zapret-autotune", Progress { step: i + 1, total, title: title.clone() });
        let mut running = None;
        let mut error = None;
        if let Some(raw) = &args {
            let exited = Arc::new(AtomicBool::new(false));
            let built = build_args(raw, &dir, hostlist.as_deref());
            match child::spawn(
                app,
                Spawn {
                    exe: &exe,
                    args: built.iter().map(|a| a.as_os_str()).collect(),
                    cwd: Some(&dir),
                    source: "dpi",
                    parse: parse_line,
                    exited: exited.clone(),
                },
                |_app, _code| async {},
            )
            .await
            {
                Ok(r) => {
                    tokio::time::sleep(Duration::from_millis(1200)).await;
                    if exited.load(Ordering::SeqCst) {
                        error = Some("не запустилось — проверьте параметры".to_string());
                    }
                    running = Some(r);
                }
                Err(e) => error = Some(format!("{e:#}")),
            }
        }
        let (ok, failed) = if error.is_some() { (0, domains.clone()) } else { probe_all(&domains).await };
        if let Some(r) = running {
            r.stop().await;
            // Драйверу нужно мгновение, чтобы снять фильтры до следующего запуска.
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
        log(app, "info", format!("Подбор: «{title}» — {ok} из {} доменов", domains.len()));
        out.push(ProbeResult { id, title, ok, total: domains.len(), failed, error });
    }

    if was_running {
        start(app).await?;
    }
    Ok(out)
}
