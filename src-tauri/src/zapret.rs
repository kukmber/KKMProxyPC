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

/// Подставляет пути к файлам-заготовкам, лежащим рядом с winws.exe.
fn build_args(s: &Strategy, dir: &Path) -> Vec<OsString> {
    s.args
        .iter()
        .map(|a| match a.split_once("=@") {
            Some((key, rel)) => {
                let mut v = OsString::from(key);
                v.push("=");
                v.push(dir.join(rel));
                v
            }
            None => OsString::from(a),
        })
        .collect()
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
    let s = {
        let set = st.settings.lock().unwrap();
        strategy(&set.zapret_strategy)
    };
    let args = build_args(s, &dir);

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
            strategy: Some(s.title.into()),
        }
    });
    log(app, "info", format!("Обход блокировок включён, стратегия «{}»", s.title));
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

/// Проверяет стратегию без запуска: winws разбирает параметры и выходит.
pub async fn check(app: &AppHandle, id: &str) -> Result<()> {
    let exe = cores::ensure(app, "zapret").await?;
    let dir = exe.parent().context("нет папки ядра")?.to_path_buf();
    let mut args = build_args(strategy(id), &dir);
    args.insert(0, OsString::from("--dry-run"));
    let out = tokio::process::Command::new(&exe)
        .args(&args)
        .current_dir(&dir)
        .creation_flags(winsys::CREATE_NO_WINDOW)
        .output()
        .await?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!("{}", err.lines().last().unwrap_or("ошибка в параметрах стратегии").trim());
    }
    Ok(())
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

    #[test]
    fn fake_files_get_absolute_paths() {
        let args = build_args(strategy("general"), Path::new("C:\\cores\\zapret"));
        let tls = args.iter().find(|a| a.to_string_lossy().starts_with("--dpi-desync-fake-tls=")).unwrap();
        assert_eq!(
            tls.to_string_lossy(),
            "--dpi-desync-fake-tls=C:\\cores\\zapret\\fake/tls_clienthello_www_google_com.bin"
        );
        // Обычные параметры не трогаем.
        assert!(args.iter().any(|a| a == "--dpi-desync-autottl=2"));
    }

    #[test]
    fn unknown_strategy_falls_back() {
        assert_eq!(strategy("нет такой").id, "general");
    }
}
