use crate::vpn::{self, VpnStatus};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

pub struct TrayItems {
    toggle: MenuItem<Wry>,
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Открыть KKMProxy", true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", "Подключить VPN", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Выход", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &toggle, &PredefinedMenuItem::separator(app)?, &quit])?;

    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("KKMProxy — VPN отключён")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "toggle" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let running = app.state::<crate::state::AppState>().vpn.status().state != "stopped";
                    let _ = if running { vpn::stop(&app).await } else { vpn::start(&app).await };
                });
            }
            "quit" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = vpn::stop(&app).await;
                    app.exit(0);
                });
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    app.manage(TrayItems { toggle });
    Ok(())
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

pub fn refresh(app: &AppHandle, s: &VpnStatus) {
    let (label, tip) = match s.state.as_str() {
        "running" => ("Отключить VPN", "KKMProxy — VPN подключён"),
        "starting" => ("Отключить VPN", "KKMProxy — подключение…"),
        "stopping" => ("Подключить VPN", "KKMProxy — отключение…"),
        _ => ("Подключить VPN", "KKMProxy — VPN отключён"),
    };
    if let Some(items) = app.try_state::<TrayItems>() {
        let _ = items.toggle.set_text(label);
    }
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(tip));
    }
}
