//! Локальный MTProto-прокси для Telegram поверх WebSocket (tg-ws-proxy-rs).
//!
//! Приложение подбирает порт и секрет, запускает ядро и отдаёт ссылку
//! `tg://proxy?…`, которую Telegram открывает одним нажатием.

use crate::child::{self, Running, Spawn};
use crate::state::AppState;
use crate::util::{self, now_ms};
use crate::{cores, winsys};
use anyhow::{bail, Result};
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
fn link_for(port: u16, secret: &str) -> String {
    format!("tg://proxy?server=127.0.0.1&port={port}&secret=dd{secret}")
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
    let (want_port, secret) = {
        let s = st.settings.lock().unwrap();
        (s.tg_port, s.tg_secret.clone())
    };
    let port = if util::port_is_free(want_port) { want_port } else { util::free_port()? };
    // Секрет держим постоянным: иначе после каждого перезапуска ссылку
    // пришлось бы заново применять в Telegram.
    let secret = match secret {
        Some(s) if s.len() == 32 && s.chars().all(|c| c.is_ascii_hexdigit()) => s,
        _ => {
            let s = util::random_hex(16);
            st.update_settings(|set| set.tg_secret = Some(s.clone()))?;
            s
        }
    };

    let args: Vec<OsString> = vec![
        "--port".into(),
        port.to_string().into(),
        // Слушаем только себя: прокси нужен этому компьютеру, наружу его открывать незачем.
        "--host".into(),
        "127.0.0.1".into(),
        // Без этого ядро подставляет в ссылку самостоятельно определённый адрес — и когда включён
        // наш же VPN, им оказывается адрес из диапазона fake-ip (198.18.x.x).
        "--link-ip".into(),
        "127.0.0.1".into(),
        "--secret".into(),
        secret.clone().into(),
        // Список доменов Cloudflare: запасной путь, когда адреса Telegram режут.
        "--default-domains".into(),
    ];

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
            port: Some(port),
            secret: Some(secret.clone()),
            link: Some(link_for(port, &secret)),
            started_at: Some(now_ms()),
            error: None,
        }
    });
    log(app, "info", format!("Прокси для Telegram слушает 127.0.0.1:{port}"));
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
            link_for(1443, "00112233445566778899aabbccddeeff"),
            "tg://proxy?server=127.0.0.1&port=1443&secret=dd00112233445566778899aabbccddeeff"
        );
    }
}
