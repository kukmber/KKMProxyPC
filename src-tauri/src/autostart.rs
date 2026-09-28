//! Запуск вместе с Windows.
//!
//! Обычная запись в реестре (`...\CurrentVersion\Run`) всегда стартует программу
//! **без прав администратора**. Режиму TUN и обходу блокировок права нужны, и при
//! таком запуске они молча не поднимаются. Поэтому, когда права есть, автозапуск
//! делается задачей в планировщике с флагом «с наивысшими правами», а запись в
//! реестре остаётся запасным вариантом.

use anyhow::{bail, Result};
use serde::Serialize;
use std::os::windows::process::CommandExt;
use std::process::Command;
use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

const TASK: &str = "KKMProxy";

#[derive(Serialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// Автозапуска нет.
    Off,
    /// Задача планировщика: стартует с правами администратора.
    Task,
    /// Запись в реестре: стартует без прав.
    Registry,
}

fn schtasks(args: &[&str]) -> Result<std::process::Output> {
    Ok(Command::new("schtasks.exe")
        .args(args)
        .creation_flags(crate::winsys::CREATE_NO_WINDOW)
        .output()?)
}

fn task_exists() -> bool {
    schtasks(&["/Query", "/TN", TASK]).map_or(false, |o| o.status.success())
}

pub fn status(app: &AppHandle) -> Kind {
    if task_exists() {
        Kind::Task
    } else if app.autolaunch().is_enabled().unwrap_or(false) {
        Kind::Registry
    } else {
        Kind::Off
    }
}

/// Включает автозапуск. С правами администратора создаётся задача планировщика,
/// без них — обычная запись в реестре.
pub fn enable(app: &AppHandle) -> Result<Kind> {
    disable(app)?;
    if crate::winsys::is_elevated() {
        let exe = std::env::current_exe()?;
        // Кавычки внутри /TR обязательны: в пути есть пробелы.
        let run = format!("\"{}\" --autostart", exe.display());
        let out = schtasks(&["/Create", "/TN", TASK, "/TR", &run, "/SC", "ONLOGON", "/RL", "HIGHEST", "/F"])?;
        if out.status.success() {
            return Ok(Kind::Task);
        }
        // Планировщик может быть недоступен — тогда хотя бы обычный автозапуск.
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        app.autolaunch()
            .enable()
            .map_err(|e| anyhow::anyhow!("{err}; и запись в реестре не вышла: {e}"))?;
        return Ok(Kind::Registry);
    }
    app.autolaunch()
        .enable()
        .map_err(|e| anyhow::anyhow!("не удалось включить автозапуск: {e}"))?;
    Ok(Kind::Registry)
}

pub fn disable(app: &AppHandle) -> Result<()> {
    if task_exists() {
        let out = schtasks(&["/Delete", "/TN", TASK, "/F"])?;
        if !out.status.success() {
            // Задачу с наивысшими правами удаляет только администратор.
            if !crate::winsys::is_elevated() {
                bail!("Убрать автозапуск можно только от имени администратора — он создавался с правами");
            }
            bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
        }
    }
    let _ = app.autolaunch().disable();
    Ok(())
}
