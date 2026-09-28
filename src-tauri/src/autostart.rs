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

/// Включает автозапуск. Задача планировщика умеет стартовать с правами
/// администратора, поэтому создаём именно её. Если прав сейчас нет, Windows
/// один раз спросит подтверждение — перезапускать всю программу не нужно.
pub fn enable(app: &AppHandle) -> Result<Kind> {
    disable(app)?;
    let exe = std::env::current_exe()?;
    // Кавычки внутри /TR обязательны: в пути есть пробелы.
    let run = format!("\"{}\" --autostart", exe.display());
    let args = ["/Create", "/TN", TASK, "/TR", &run, "/SC", "ONLOGON", "/RL", "HIGHEST", "/F"];

    let created = if crate::winsys::is_elevated() {
        schtasks(&args).map_or(false, |o| o.status.success())
    } else {
        // Через ShellExecute аргументы идут одной строкой, поэтому кавычим сами.
        let line = format!("/Create /TN {TASK} /TR \"\\\"{}\\\" --autostart\" /SC ONLOGON /RL HIGHEST /F", exe.display());
        crate::winsys::run_elevated_wait("schtasks.exe", &line, 60_000).map_or(false, |code| code == 0)
    };
    if created && task_exists() {
        return Ok(Kind::Task);
    }

    // Не вышло — пусть будет хотя бы обычный автозапуск, без прав.
    app.autolaunch()
        .enable()
        .map_err(|e| anyhow::anyhow!("не удалось включить автозапуск: {e}"))?;
    Ok(Kind::Registry)
}

pub fn disable(app: &AppHandle) -> Result<()> {
    if task_exists() {
        let removed = if crate::winsys::is_elevated() {
            schtasks(&["/Delete", "/TN", TASK, "/F"]).map_or(false, |o| o.status.success())
        } else {
            // Задачу с наивысшими правами удаляет только администратор.
            crate::winsys::run_elevated_wait("schtasks.exe", &format!("/Delete /TN {TASK} /F"), 60_000)
                .map_or(false, |code| code == 0)
        };
        if !removed && task_exists() {
            bail!("Не удалось убрать автозапуск: нужно подтверждение прав администратора");
        }
    }
    let _ = app.autolaunch().disable();
    Ok(())
}
