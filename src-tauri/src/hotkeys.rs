//! Горячие клавиши, работающие поверх любой программы.
//!
//! Нужны, чтобы включить VPN или обход, не отрываясь от игры или звонка:
//! окно для этого открывать не приходится.

use crate::state::AppState;
use crate::{tgproxy, vpn, zapret};
use anyhow::{bail, Result};
use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

/// Что можно повесить на клавишу.
pub const ACTIONS: &[(&str, &str)] = &[
    ("vpn", "Включить или выключить VPN"),
    ("tg", "Включить или выключить прокси для Telegram"),
    ("zapret", "Включить или выключить обход блокировок"),
    ("restart", "Перезапустить всё включённое"),
    ("window", "Показать или спрятать окно"),
];

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyInfo {
    pub action: String,
    pub about: String,
    pub accelerator: String,
}

pub fn list(app: &AppHandle) -> Vec<HotkeyInfo> {
    let keys = app.state::<AppState>().settings.lock().unwrap().hotkeys.clone();
    ACTIONS
        .iter()
        .map(|(action, about)| HotkeyInfo {
            action: (*action).into(),
            about: (*about).into(),
            accelerator: keys.get(*action).cloned().unwrap_or_default(),
        })
        .collect()
}

/// Перерегистрирует все сочетания из настроек. Вызывается при запуске и после
/// каждого изменения: снимаем всё и ставим заново, чтобы не следить за тем,
/// что именно поменялось.
pub fn apply(app: &AppHandle) -> Result<()> {
    let manager = app.global_shortcut();
    let _ = manager.unregister_all();
    let keys = app.state::<AppState>().settings.lock().unwrap().hotkeys.clone();
    for (action, accel) in keys {
        if accel.trim().is_empty() {
            continue;
        }
        let Ok(shortcut) = accel.parse::<Shortcut>() else {
            log(app, "warning", format!("Не понял сочетание «{accel}» — пропускаю"));
            continue;
        };
        let handle = app.clone();
        let act = action.clone();
        if let Err(e) = manager.on_shortcut(shortcut, move |_app, _sc, event| {
            // Реагируем на нажатие, иначе действие сработает дважды.
            if event.state == ShortcutState::Pressed {
                run(&handle, &act);
            }
        }) {
            log(app, "warning", format!("Сочетание «{accel}» занято другой программой: {e}"));
        }
    }
    Ok(())
}

/// Сохраняет сочетание для действия. Пустая строка убирает его.
pub fn set(app: &AppHandle, action: &str, accelerator: &str) -> Result<()> {
    if !ACTIONS.iter().any(|(a, _)| *a == action) {
        bail!("неизвестное действие {action}");
    }
    let accel = accelerator.trim().to_string();
    if !accel.is_empty() {
        if accel.parse::<Shortcut>().is_err() {
            bail!("Такое сочетание не подходит");
        }
        // Одно сочетание на два действия — почти наверняка ошибка.
        let busy = app
            .state::<AppState>()
            .settings
            .lock()
            .unwrap()
            .hotkeys
            .iter()
            .any(|(a, v)| a != action && v.eq_ignore_ascii_case(&accel));
        if busy {
            bail!("Это сочетание уже занято другим действием");
        }
    }
    let st = app.state::<AppState>();
    st.update_settings(|s| {
        if accel.is_empty() {
            s.hotkeys.remove(action);
        } else {
            s.hotkeys.insert(action.to_string(), accel.clone());
        }
    })?;
    apply(app)
}

fn log(app: &AppHandle, level: &str, msg: impl Into<String>) {
    app.state::<AppState>().logs.push(app, "app", level, msg);
}

/// Выполняет действие горячей клавиши. Всё, что включено, — выключает,
/// и наоборот: одна клавиша служит и включением, и выключением.
fn run(app: &AppHandle, action: &str) {
    let app = app.clone();
    let action = action.to_string();
    tauri::async_runtime::spawn(async move {
        let st = app.state::<AppState>();
        let result: Result<&str> = match action.as_str() {
            "vpn" => {
                if st.vpn.status().state == "stopped" {
                    vpn::start(&app).await.map(|_| "VPN включён")
                } else {
                    vpn::stop(&app).await.map(|_| "VPN выключен")
                }
            }
            "tg" => {
                if st.tg.status().state == "stopped" {
                    tgproxy::start(&app).await.map(|_| "Прокси для Telegram включён")
                } else {
                    tgproxy::stop(&app).await.map(|_| "Прокси для Telegram выключен")
                }
            }
            "zapret" => {
                if st.zapret.status().state == "stopped" {
                    zapret::start(&app).await.map(|_| "Обход блокировок включён")
                } else {
                    zapret::stop(&app).await.map(|_| "Обход блокировок выключен")
                }
            }
            "restart" => {
                let mut done = Vec::new();
                if st.vpn.status().state == "running" {
                    let _ = vpn::restart(&app).await;
                    done.push("VPN");
                }
                if st.tg.status().state == "running" {
                    let _ = tgproxy::stop(&app).await;
                    let _ = tgproxy::start(&app).await;
                    done.push("Telegram");
                }
                if st.zapret.status().state == "running" {
                    let _ = zapret::stop(&app).await;
                    let _ = zapret::start(&app).await;
                    done.push("Zapret");
                }
                Ok(if done.is_empty() { "Перезапускать нечего" } else { "Перезапущено" })
            }
            "window" => {
                if let Some(w) = app.get_webview_window("main") {
                    if w.is_visible().unwrap_or(false) {
                        let _ = w.hide();
                    } else {
                        crate::tray::show_main(&app);
                    }
                }
                Ok("")
            }
            _ => Ok(""),
        };
        match result {
            Ok(msg) if !msg.is_empty() => log(&app, "info", format!("Горячая клавиша: {msg}")),
            Err(e) => log(&app, "error", format!("Горячая клавиша: {e:#}")),
            _ => {}
        }
    });
}
