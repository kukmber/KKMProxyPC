//! Локальный MTProto-прокси для Telegram поверх WebSocket (tg-ws-proxy-rs).
//!
//! Приложение подбирает порт и секрет, запускает ядро и отдаёт ссылку
//! `tg://proxy?…`, которую Telegram открывает одним нажатием.

use crate::child::{self, Running, Spawn};
use crate::settings::TgAdvanced;
use crate::state::AppState;
use crate::util::{self, now_ms};
use crate::{cores, winsys};
use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use std::ffi::OsString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct TgStatus {
    /// stopped | starting | running | stopping
    pub state: String,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub secret: Option<String>,
    /// Готовая ссылка tg://proxy?… с префиксом dd (режим padded).
    pub link: Option<String>,
    pub started_at: Option<u64>,
    pub error: Option<String>,
}

pub struct TgManager {
    status: Mutex<TgStatus>,
    running: tokio::sync::Mutex<Option<Running>>,
    op: tokio::sync::Mutex<()>,
}

impl TgManager {
    pub fn new() -> Self {
        Self {
            status: Mutex::new(TgStatus { state: "stopped".into(), ..Default::default() }),
            running: tokio::sync::Mutex::new(None),
            op: tokio::sync::Mutex::new(()),
        }
    }

    pub fn status(&self) -> TgStatus {
        self.status.lock().unwrap().clone()
    }
}

fn set_status(app: &AppHandle, f: impl FnOnce(&mut TgStatus)) {
    let st = app.state::<AppState>();
    let snapshot = {
        let mut s = st.tg.status.lock().unwrap();
        f(&mut s);
        s.clone()
    };
    let _ = app.emit("tg-status", &snapshot);
}

fn log(app: &AppHandle, level: &str, msg: impl Into<String>) {
    app.state::<AppState>().logs.push(app, "tg", level, msg);
}

/// Секрет в ссылке идёт с префиксом `dd` — так Telegram включает режим padded,
/// который труднее отличить по длине пакетов.
fn link_for(host: &str, port: u16, secret: &str) -> String {
    format!("tg://proxy?server={host}&port={port}&secret=dd{secret}")
}

/// Адрес этого компьютера в локальной сети. Узнаётся без отправки данных:
/// системе достаточно выбрать исходящий интерфейс для UDP-сокета.
fn lan_ip() -> Option<String> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:53").ok()?;
    Some(sock.local_addr().ok()?.ip().to_string())
}

/// Что подставлять в ссылку: сам адрес прослушивания, а для 0.0.0.0 —
/// адрес компьютера в локальной сети, иначе ссылка никуда не приведёт.
fn link_host(host: &str) -> String {
    if host == "0.0.0.0" {
        lan_ip().unwrap_or_else(|| "127.0.0.1".into())
    } else {
        host.to_string()
    }
}

/// Создаёт новый случайный секрет и сохраняет его.
pub fn regenerate_secret(app: &AppHandle) -> Result<String> {
    let secret = util::random_hex(16);
    app.state::<AppState>().update_settings(|s| s.tg_secret = Some(secret.clone()))?;
    Ok(secret)
}

pub fn valid_secret(s: &str) -> bool {
    s.len() == 32 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Домены из текстового поля: по одному в строке или через запятую.
fn split_domains(raw: &str) -> Vec<String> {
    raw.split([',', '\n', '\r', ' ', '\t', ';'])
        .map(|d| d.trim().trim_start_matches("*.").to_string())
        .filter(|d| !d.is_empty())
        .collect()
}

/// Свои адреса DC из строк вида `2:149.154.167.220`.
fn dc_targets(raw: &str) -> Vec<String> {
    raw.split(['\n', '\r', ',', ' ', '\t'])
        .map(|l| l.trim().to_string())
        .filter(|l| match l.split_once(':') {
            // Адрес может быть и IPv6 — тогда двоеточий в нём много.
            Some((dc, ip)) => dc.parse::<i32>().is_ok() && ip.parse::<std::net::IpAddr>().is_ok(),
            None => false,
        })
        .collect()
}

/// Аргументы ядра из «продвинутого» блока. Всё, что не отмечено, не передаём —
/// тогда ядро ведёт себя так, как задумано его авторами.
fn advanced_args(a: &TgAdvanced) -> Vec<OsString> {
    let mut out: Vec<OsString> = Vec::new();
    let cf = split_domains(&a.cf_domains);

    if a.cf_proxy && !cf.is_empty() {
        out.push("--cf-domain".into());
        out.push(cf.join(",").into());
    }
    // Публичный список Flowseal: без него свои домены — единственный путь.
    if a.default_domains {
        out.push("--default-domains".into());
    }
    let worker = split_domains(&a.cf_worker_domains);
    if a.cf_worker && !worker.is_empty() {
        out.push("--cf-worker-domain".into());
        out.push(worker.join(",").into());
    }
    // Балансировка имеет смысл только когда доменов больше одного.
    if a.cf_balance && cf.len() > 1 {
        out.push("--cf-balance".into());
    }
    if a.no_h2 {
        out.push("--no-h2".into());
    }
    // Эти две настройки относятся только к Cloudflare — без доменов бесполезны.
    if a.cf_disable_tls && (a.cf_proxy || a.cf_worker) {
        out.push("--cf-disable-tls".into());
    }
    if a.verbose {
        out.push("--verbose".into());
    }
    if a.buf_kb > 0 {
        out.push("--buf-kb".into());
        out.push(a.buf_kb.to_string().into());
    }
    if a.pool_size > 0 {
        out.push("--pool-size".into());
        out.push(a.pool_size.to_string().into());
    }
    for dc in dc_targets(&a.dc_ips) {
        out.push("--dc-ip".into());
        out.push(dc.into());
    }
    out
}

/// Убирает цветовые коды из вывода ядра — в окне проверки они лишние.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.next() == Some('[') {
                for code in chars.by_ref() {
                    if code.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Проверяет домены Cloudflare (`--check`) и возвращает то, что ответило ядро.
/// kind: cfproxy — свои домены Cloudflare, worker — домен Cloudflare Worker.
/// `domains` — то, что сейчас в поле ввода; если пусто, берём сохранённое.
pub async fn check(app: &AppHandle, kind: &str, domains: &str) -> Result<String> {
    let exe = cores::ensure(app, "tgws").await?;
    let advanced = app.state::<AppState>().settings.lock().unwrap().tg_advanced.clone();
    let typed = domains.trim().to_string();
    let mut args: Vec<OsString> = vec!["--check".into()];
    match kind {
        "cfproxy" => {
            let list = split_domains(if typed.is_empty() { &advanced.cf_domains } else { &typed });
            if list.is_empty() {
                bail!("Сначала впишите домены Cloudflare");
            }
            args.push("--cf-domain".into());
            args.push(list.join(",").into());
        }
        "worker" => {
            let list = split_domains(if typed.is_empty() { &advanced.cf_worker_domains } else { &typed });
            if list.is_empty() {
                bail!("Сначала впишите домен Cloudflare Worker");
            }
            args.push("--cf-worker-domain".into());
            args.push(list.join(",").into());
        }
        _ => bail!("неизвестная проверка"),
    }
    if advanced.cf_disable_tls {
        args.push("--cf-disable-tls".into());
    }

    let output = tokio::time::timeout(
        Duration::from_secs(60),
        tokio::process::Command::new(&exe)
            .args(&args)
            .creation_flags(winsys::CREATE_NO_WINDOW)
            .output(),
    )
    .await
    .map_err(|_| anyhow!("ядро не ответило за 60 секунд"))??;

    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    let text = strip_ansi(&text)
        .lines()
        .map(|l| l.trim_end().to_string())
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_empty() {
        bail!("ядро ничего не ответило — проверьте домены вручную");
    }
    Ok(text)
}

pub async fn start(app: &AppHandle) -> Result<()> {
    let st = app.state::<AppState>();
    let _op = st.tg.op.lock().await;
    if st.tg.status().state == "running" {
        return Ok(());
    }
    set_status(app, |s| *s = TgStatus { state: "starting".into(), ..Default::default() });
    match start_inner(app).await {
        Ok(()) => Ok(()),
        Err(e) => {
            let msg = format!("{e:#}");
            log(app, "error", format!("Не удалось запустить прокси: {msg}"));
            if let Some(r) = st.tg.running.lock().await.take() {
                r.stop().await;
            }
            set_status(app, |s| *s = TgStatus { state: "stopped".into(), error: Some(msg), ..Default::default() });
            Err(e)
        }
    }
}

async fn start_inner(app: &AppHandle) -> Result<()> {
    let st = app.state::<AppState>();
    let exe = cores::ensure(app, "tgws").await?;
    let (host, want_port, secret, advanced) = {
        let s = st.settings.lock().unwrap();
        (s.tg_host.clone(), s.tg_port, s.tg_secret.clone(), s.tg_advanced.clone())
    };
    let port = if util::port_is_free(want_port) { want_port } else { util::free_port()? };
    // Секрет держим постоянным: иначе после каждого перезапуска ссылку
    // пришлось бы заново применять в Telegram.
    let secret = match secret {
        Some(s) if valid_secret(&s) => s,
        _ => {
            let s = util::random_hex(16);
            st.update_settings(|set| set.tg_secret = Some(s.clone()))?;
            s
        }
    };

    let args: Vec<OsString> = {
        let mut args: Vec<OsString> = vec![
            "--port".into(),
            port.to_string().into(),
            // По умолчанию слушаем только себя; 0.0.0.0 открывает прокси для устройств в сети.
            "--host".into(),
            host.clone().into(),
            // Без этого ядро подставляет в ссылку самостоятельно определённый адрес — и когда включён
            // наш же VPN, им оказывается адрес из диапазона fake-ip (198.18.x.x).
            "--link-ip".into(),
            link_host(&host).into(),
            "--secret".into(),
            secret.clone().into(),
        ];
        // Всё, что отмечено в «продвинутом» блоке: домены Cloudflare, Worker, DC→IP.
        args.extend(advanced_args(&advanced));
        args
    };

    let exited = Arc::new(AtomicBool::new(false));
    let running = child::spawn(
        app,
        Spawn {
            exe: &exe,
            args: args.iter().map(|a| a.as_os_str()).collect(),
            cwd: exe.parent(),
            source: "tg",
            parse: parse_line,
            exited: exited.clone(),
        },
        |app, code| async move { on_crash(&app, code).await },
    )
    .await?;
    *st.tg.running.lock().await = Some(running);

    wait_ready(port, &exited).await?;
    set_status(app, |s| {
        *s = TgStatus {
            state: "running".into(),
            host: Some(host.clone()),
            port: Some(port),
            secret: Some(secret.clone()),
            link: Some(link_for(&link_host(&host), port, &secret)),
            started_at: Some(now_ms()),
            error: None,
        }
    });
    log(app, "info", format!("Прокси для Telegram слушает {host}:{port}"));
    Ok(())
}

pub async fn stop(app: &AppHandle) -> Result<()> {
    let st = app.state::<AppState>();
    let _op = st.tg.op.lock().await;
    if st.tg.status().state == "stopped" {
        return Ok(());
    }
    set_status(app, |s| s.state = "stopping".into());
    if let Some(r) = st.tg.running.lock().await.take() {
        r.stop().await;
    }
    set_status(app, |s| *s = TgStatus { state: "stopped".into(), ..Default::default() });
    log(app, "info", "Прокси для Telegram остановлен");
    Ok(())
}

async fn on_crash(app: &AppHandle, code: Option<i32>) {
    let st = app.state::<AppState>();
    if st.tg.status().state != "running" {
        return; // ошибку при запуске покажет start()
    }
    st.tg.running.lock().await.take();
    let msg = format!("Прокси неожиданно завершился (код {})", code.map_or("?".into(), |c| c.to_string()));
    log(app, "error", &msg);
    set_status(app, |s| *s = TgStatus { state: "stopped".into(), error: Some(msg), ..Default::default() });
}

/// Готовность — принятое TCP-соединение на порт: ядро уже слушает.
async fn wait_ready(port: u16, exited: &AtomicBool) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if exited.load(Ordering::SeqCst) {
            bail!("ядро завершилось при запуске — подробности в журнале");
        }
        if tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            return Ok(());
        }
        if Instant::now() > deadline {
            bail!("ядро не начало слушать порт за 30 секунд");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Сколько клиентов Telegram сейчас подключено. Своей статистики ядро не отдаёт,
/// поэтому считаем установленные соединения на его порт через Windows.
pub async fn connections(app: &AppHandle) -> usize {
    let st = app.state::<AppState>();
    let Some(port) = st.tg.status().port else { return 0 };
    let guard = st.tg.running.lock().await;
    guard.as_ref().map_or(0, |r| winsys::established_connections(r.pid, port))
}

/// Разбирает строку ядра вида
/// `2026-09-24T13:19:54.923336Z  INFO tg_ws_proxy_rs::server: Listening on …`.
fn parse_line(line: &str) -> (String, String) {
    let rest = line.split_once('Z').map_or(line, |(_, r)| r).trim_start();
    let (level, rest) = match rest.split_once(char::is_whitespace) {
        Some((lvl, r)) if matches!(lvl, "TRACE" | "DEBUG" | "INFO" | "WARN" | "ERROR") => {
            let level = match lvl {
                "WARN" => "warning",
                "ERROR" => "error",
                "TRACE" => "debug",
                other => other,
            };
            (level.to_ascii_lowercase(), r)
        }
        _ => ("info".to_string(), rest),
    };
    // Отрезаем имя модуля («tg_ws_proxy_rs::server: ») — в журнале от него нет пользы.
    let msg = rest
        .trim_start()
        .split_once(": ")
        .filter(|(m, _)| m.contains("::") && !m.contains(' '))
        .map_or(rest, |(_, m)| m);
    (level, msg.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_core_log() {
        let (lvl, msg) = parse_line("2026-09-24T13:19:54.923336Z  INFO tg_ws_proxy_rs::server:   Listening on   127.0.0.1:18443");
        assert_eq!(lvl, "info");
        assert_eq!(msg, "Listening on   127.0.0.1:18443");
        let (lvl, msg) = parse_line("2026-09-24T13:20:44.908669Z  WARN tg_ws_proxy_rs::server:   Link shows 127.0.0.1");
        assert_eq!(lvl, "warning");
        assert_eq!(msg, "Link shows 127.0.0.1");
        // Строка без метки времени не должна теряться.
        assert_eq!(parse_line("plain text").1, "plain text");
    }

    #[test]
    fn link_uses_padded_prefix() {
        assert_eq!(
            link_for("127.0.0.1", 1443, "00112233445566778899aabbccddeeff"),
            "tg://proxy?server=127.0.0.1&port=1443&secret=dd00112233445566778899aabbccddeeff"
        );
    }

    fn strings(args: Vec<OsString>) -> Vec<String> {
        args.into_iter().map(|a| a.into_string().unwrap()).collect()
    }

    /// По умолчанию ядру достаётся только публичный список доменов Flowseal.
    #[test]
    fn default_advanced_args_are_minimal() {
        assert_eq!(strings(advanced_args(&TgAdvanced::default())), vec!["--default-domains"]);
    }

    #[test]
    fn builds_advanced_args() {
        let a = TgAdvanced {
            cf_proxy: true,
            cf_domains: "a.example.com, b.example.com\n*.c.example.com".into(),
            cf_balance: true,
            default_domains: false,
            no_h2: true,
            verbose: true,
            pool_size: 8,
            dc_ips: "2:149.154.167.220\nмусор\n4:2001:db8::1".into(),
            ..Default::default()
        };
        let args = strings(advanced_args(&a));
        assert_eq!(args[0], "--cf-domain");
        assert_eq!(args[1], "a.example.com,b.example.com,c.example.com");
        assert!(args.contains(&"--cf-balance".to_string()));
        assert!(args.contains(&"--no-h2".to_string()));
        assert!(args.contains(&"--verbose".to_string()));
        assert!(args.contains(&"--pool-size".to_string()));
        assert!(!args.contains(&"--default-domains".to_string()));
        // Мусорная строка отброшена, IPv6 разобран целиком.
        assert_eq!(args.iter().filter(|a| *a == "--dc-ip").count(), 2);
        assert!(args.contains(&"4:2001:db8::1".to_string()));
    }
}
