mod child;
mod commands;
mod config;
mod cores;
mod hostsets;
mod hotkeys;
mod logs;
mod paths;
mod profiles;
mod settings;
mod state;
mod subscription;
mod tgproxy;
mod tray;
mod util;
mod vpn;
mod zapret;
mod winsys;

use tauri::{Manager, RunEvent, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let args: Vec<String> = std::env::args().collect();
    // После перезапуска с правами администратора ждём, пока закроется старая копия.
    if let Some(pid) = args
        .iter()
        .position(|a| a == "--wait-pid")
        .and_then(|i| args.get(i + 1))
        .and_then(|p| p.parse().ok())
    {
        winsys::wait_for_pid(pid, 10_000);
    }
    let autoconnect = args.iter().any(|a| a == "--connect");
    // --autostart добавляет Windows при запуске вместе с системой.
    let autostarted = args.iter().any(|a| a == "--autostart");
    let minimized = autostarted || args.iter().any(|a| a == "--minimized");

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| tray::show_main(app)))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(move |app| {
            let root = app.path().app_local_data_dir()?;
            let resources = app.path().resource_dir().ok();
            app.manage(state::AppState::new(paths::Paths::new(root, resources)?));
            vpn::restore_sysproxy(app.handle());
            tray::build(app.handle())?;
            cores::schedule_checks(app.handle());
            if let Err(e) = hotkeys::apply(app.handle()) {
                eprintln!("горячие клавиши не применились: {e:#}");
            }

            if let Some(w) = app.get_webview_window("main") {
                if winsys::windows_build() >= 22000 {
                    let _ = w.set_effects(
                        tauri::window::EffectsBuilder::new().effect(tauri::window::Effect::Mica).build(),
                    );
                }
                let hide_at_start = minimized
                    && {
                        let st = app.state::<state::AppState>();
                        let s = st.settings.lock().unwrap();
                        s.start_minimized && s.tray_enabled
                    };
                if !hide_at_start {
                    let _ = w.show();
                }
            }
            // Что поднять сразу: явно переданное --connect либо отмеченное в настройках.
            let auto = {
                let st = app.state::<state::AppState>();
                let s = st.settings.lock().unwrap();
                (autoconnect || s.autostart_vpn, s.autostart_tg, s.autostart_zapret)
            };
            if auto.0 || auto.1 || auto.2 {
                let app = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    if auto.0 {
                        let _ = vpn::start(&app).await;
                    }
                    if auto.1 {
                        let _ = tgproxy::start(&app).await;
                    }
                    if auto.2 {
                        let _ = zapret::start(&app).await;
                    }
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Крестик прячет окно в трей; выход — из меню значка.
            if let WindowEvent::CloseRequested { api, .. } = event {
                let to_tray = window
                    .app_handle()
                    .state::<state::AppState>()
                    .settings
                    .lock()
                    .unwrap()
                    .tray_enabled;
                if to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                }
                // Иначе окно закрывается обычным образом и программа завершается.
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::vpn_status,
            commands::vpn_start,
            commands::vpn_stop,
            commands::get_settings,
            commands::set_mode,
            commands::set_connection,
            commands::list_profiles,
            commands::add_profile_url,
            commands::add_profile_content,
            commands::refresh_profile,
            commands::delete_profile,
            commands::rename_profile,
            commands::set_active_profile,
            commands::get_logs,
            commands::clear_logs,
            commands::platform_info,
            commands::restart_as_admin,
            commands::core_info,
            commands::check_core_updates,
            commands::update_core,
            commands::tg_status,
            commands::tg_start,
            commands::tg_stop,
            commands::tg_connections,
            commands::set_tg_params,
            commands::regenerate_tg_secret,
            commands::zapret_status,
            commands::zapret_start,
            commands::zapret_stop,
            commands::zapret_strategies,
            commands::set_zapret_strategy,
            commands::zapret_hosts,
            commands::set_zapret_hosts,
            commands::set_zapret_custom,
            commands::zapret_autotune,
            commands::dpi_conflict,
            commands::app_rules,
            commands::set_app_rules,
            commands::running_processes,
            commands::hotkeys,
            commands::set_hotkey,
            commands::restart_all,
            commands::get_startup,
            commands::set_startup,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                vpn::restore_sysproxy(app);
            }
        });
}
