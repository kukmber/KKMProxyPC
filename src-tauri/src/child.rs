//! Запуск дочерних процессов-ядер: без окна консоли, с привязкой к Job Object,
//! с перекачкой вывода в журнал приложения и уведомлением о неожиданном выходе.

use crate::state::AppState;
use crate::winsys;
use anyhow::{Context, Result};
use std::ffi::OsStr;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::sync::oneshot;

/// Ручка запущенного процесса: `stop` останавливает и ждёт завершения.
pub struct Running {
    kill: oneshot::Sender<()>,
    done: oneshot::Receiver<()>,
    /// Нужен, чтобы считать подключения клиентов к порту именно этого процесса.
    pub pid: u32,
}

impl Running {
    pub async fn stop(self) {
        let _ = self.kill.send(());
        let _ = tokio::time::timeout(Duration::from_secs(5), self.done).await;
    }
}

pub struct Spawn<'a> {
    pub exe: &'a Path,
    pub args: Vec<&'a OsStr>,
    pub cwd: Option<&'a Path>,
    /// Раздел журнала: vpn | tg | dpi.
    pub source: &'static str,
    /// Разбор строки вывода ядра в пару «уровень, сообщение».
    pub parse: fn(&str) -> (String, String),
    /// Взводится, когда процесс завершился — чтобы ожидание готовности не висело зря.
    pub exited: Arc<AtomicBool>,
}

/// Запускает процесс. `on_exit` вызывается только при неожиданном завершении
/// (не после `Running::stop`) с кодом возврата.
pub async fn spawn<F, Fut>(app: &AppHandle, s: Spawn<'_>, on_exit: F) -> Result<Running>
where
    F: FnOnce(AppHandle, Option<i32>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send,
{
    let mut cmd = tokio::process::Command::new(s.exe);
    cmd.args(&s.args);
    if let Some(dir) = s.cwd {
        cmd.current_dir(dir);
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.creation_flags(winsys::CREATE_NO_WINDOW);
    cmd.kill_on_drop(true);
    let name = s.exe.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let mut child = cmd.spawn().with_context(|| format!("не удалось запустить {name}"))?;
    // Job Object: если приложение упадёт или его снимут из диспетчера задач,
    // Windows сама остановит ядро — иначе оно осталось бы висеть в памяти.
    if let Some(h) = child.raw_handle() {
        winsys::attach_to_job(h);
    }
    if let Some(out) = child.stdout.take() {
        pipe(app.clone(), out, s.source, s.parse);
    }
    if let Some(err) = child.stderr.take() {
        pipe(app.clone(), err, s.source, s.parse);
    }

    let pid = child.id().unwrap_or(0);
    let (kill_tx, kill_rx) = oneshot::channel::<()>();
    let (done_tx, done_rx) = oneshot::channel::<()>();
    let app2 = app.clone();
    let exited = s.exited;
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
            on_exit(app2, code).await;
        }
    });
    Ok(Running { kill: kill_tx, done: done_rx, pid })
}

fn pipe(
    app: AppHandle,
    stream: impl AsyncRead + Unpin + Send + 'static,
    source: &'static str,
    parse: fn(&str) -> (String, String),
) {
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
                    let (level, msg) = parse(line);
                    app.state::<AppState>().logs.push(&app, source, &level, msg);
                }
            }
        }
    });
}
