use crate::cores::{self, CoreInfo};
use crate::logs::LogLine;
use crate::profiles::{self, Profile};
use crate::settings::Settings;
use crate::state::AppState;
use crate::tgproxy::{self, TgStatus};
use crate::zapret::{self, ZapretStatus};
use crate::util::err_str;
use crate::vpn::{self, VpnStatus};
use crate::winsys;
use serde::Serialize;
use tauri::{AppHandle, State};

type R<T> = Result<T, String>;

#[tauri::command]
pub fn vpn_status(st: State<AppState>) -> VpnStatus {
    st.vpn.status()
}

#[tauri::command]
pub async fn vpn_start(app: AppHandle) -> R<()> {
    vpn::start(&app).await.map_err(err_str)
}

#[tauri::command]
pub async fn vpn_stop(app: AppHandle) -> R<()> {
    vpn::stop(&app).await.map_err(err_str)
}

#[tauri::command]
pub fn get_settings(st: State<AppState>) -> Settings {
    st.settings.lock().unwrap().clone()
}

#[tauri::command]
pub async fn set_mode(app: AppHandle, st: State<'_, AppState>, mode: String) -> R<()> {
    st.update_settings(|s| s.mode = mode.clone()).map_err(err_str)?;
    vpn::patch_mode(&app, &mode).await.map_err(err_str)
}

/// Смена способа подключения (системный прокси / TUN) требует перезапуска ядра.
#[tauri::command]
pub async fn set_connection(app: AppHandle, st: State<'_, AppState>, connection: String) -> R<()> {
    st.update_settings(|s| s.connection = connection.clone()).map_err(err_str)?;
    if st.vpn.status().state == "running" {
        vpn::restart(&app).await.map_err(err_str)?;
    }
    Ok(())
}

#[tauri::command]
pub fn list_profiles(st: State<AppState>) -> Vec<Profile> {
    st.profiles.lock().unwrap().clone()
}

#[tauri::command]
pub async fn add_profile_url(app: AppHandle, url: String) -> R<Profile> {
    profiles::add_from_url(&app, &url).await.map_err(err_str)
}

#[tauri::command]
pub fn add_profile_content(app: AppHandle, name: Option<String>, content: String) -> R<Profile> {
    profiles::add_from_content(&app, name, &content).map_err(err_str)
}

#[tauri::command]
pub async fn refresh_profile(app: AppHandle, st: State<'_, AppState>, id: String) -> R<Profile> {
    let p = profiles::refresh(&app, &id).await.map_err(err_str)?;
    let s = st.vpn.status();
    if s.state == "running" && s.profile_id.as_deref() == Some(id.as_str()) {
        vpn::restart(&app).await.map_err(err_str)?;
    }
    Ok(p)
}

#[tauri::command]
pub async fn delete_profile(app: AppHandle, st: State<'_, AppState>, id: String) -> R<()> {
    if st.vpn.status().profile_id.as_deref() == Some(id.as_str()) {
        vpn::stop(&app).await.map_err(err_str)?;
    }
    profiles::delete(&app, &id).map_err(err_str)
}

#[tauri::command]
pub fn rename_profile(app: AppHandle, id: String, name: String) -> R<()> {
    profiles::rename(&app, &id, &name).map_err(err_str)
}

#[tauri::command]
pub async fn set_active_profile(app: AppHandle, st: State<'_, AppState>, id: String) -> R<()> {
    st.update_settings(|s| s.active_profile = Some(id.clone())).map_err(err_str)?;
    if st.vpn.status().state == "running" {
        vpn::restart(&app).await.map_err(err_str)?;
    }
    Ok(())
}

#[tauri::command]
pub fn get_logs(st: State<AppState>, source: Option<String>) -> Vec<LogLine> {
    st.logs.get(source.as_deref())
}

#[tauri::command]
pub fn clear_logs(st: State<AppState>, source: Option<String>) {
    st.logs.clear(source.as_deref())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformInfo {
    pub mica: bool,
    pub elevated: bool,
    pub version: String,
}

#[tauri::command]
pub fn platform_info(app: AppHandle) -> PlatformInfo {
    PlatformInfo {
        mica: winsys::windows_build() >= 22000,
        elevated: winsys::is_elevated(),
        version: app.package_info().version.to_string(),
    }
}

/// Перезапуск с правами администратора; после перезапуска VPN подключится сам.
#[tauri::command]
pub async fn restart_as_admin(app: AppHandle, connect: bool) -> R<()> {
    let args: &[&str] = if connect { &["--connect"] } else { &[] };
    winsys::relaunch_elevated(args).map_err(err_str)?;
    let _ = vpn::stop(&app).await;
    app.exit(0);
    Ok(())
}

#[tauri::command]
pub fn core_info(app: AppHandle) -> Vec<CoreInfo> {
    cores::info(&app)
}

#[tauri::command]
pub async fn check_core_updates(app: AppHandle) -> R<Vec<CoreInfo>> {
    cores::check_updates(&app).await.map_err(err_str)
}

#[tauri::command]
pub async fn update_core(app: AppHandle, id: String) -> R<String> {
    cores::update(&app, &id).await.map_err(err_str)
}

// ---------- прокси для Telegram ----------

#[tauri::command]
pub fn tg_status(st: State<AppState>) -> TgStatus {
    st.tg.status()
}

#[tauri::command]
pub async fn tg_start(app: AppHandle) -> R<()> {
    tgproxy::start(&app).await.map_err(err_str)
}

#[tauri::command]
pub async fn tg_stop(app: AppHandle) -> R<()> {
    tgproxy::stop(&app).await.map_err(err_str)
}

#[tauri::command]
pub async fn tg_connections(app: AppHandle) -> usize {
    tgproxy::connections(&app).await
}

/// Смена порта перезапускает прокси — ссылка с прежним портом станет недействительной.
#[tauri::command]
pub async fn set_tg_port(app: AppHandle, st: State<'_, AppState>, port: u16) -> R<()> {
    st.update_settings(|s| s.tg_port = port).map_err(err_str)?;
    if st.tg.status().state == "running" {
        tgproxy::stop(&app).await.map_err(err_str)?;
        tgproxy::start(&app).await.map_err(err_str)?;
    }
    Ok(())
}

// ---------- обход блокировок (zapret) ----------

#[tauri::command]
pub fn zapret_status(st: State<AppState>) -> ZapretStatus {
    st.zapret.status()
}

#[tauri::command]
pub async fn zapret_start(app: AppHandle) -> R<()> {
    zapret::start(&app).await.map_err(err_str)
}

#[tauri::command]
pub async fn zapret_stop(app: AppHandle) -> R<()> {
    zapret::stop(&app).await.map_err(err_str)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StrategyInfo {
    pub id: String,
    pub title: String,
    pub about: String,
}

#[tauri::command]
pub fn zapret_strategies() -> Vec<StrategyInfo> {
    zapret::STRATEGIES
        .iter()
        .map(|s| StrategyInfo { id: s.id.into(), title: s.title.into(), about: s.about.into() })
        .collect()
}

#[tauri::command]
pub async fn set_zapret_strategy(app: AppHandle, st: State<'_, AppState>, id: String) -> R<()> {
    // Проверяем параметры до запуска: winws сам разберёт их и выйдет.
    zapret::check(&app, &id).await.map_err(err_str)?;
    st.update_settings(|s| s.zapret_strategy = id).map_err(err_str)?;
    if st.zapret.status().state == "running" {
        zapret::stop(&app).await.map_err(err_str)?;
        zapret::start(&app).await.map_err(err_str)?;
    }
    Ok(())
}

/// Перезапускает всё, что сейчас включено, — по кнопке «Перезапустить всё».
#[tauri::command]
pub async fn restart_all(app: AppHandle, st: State<'_, AppState>) -> R<Vec<String>> {
    let (vpn_on, tg_on, dpi_on) = (
        st.vpn.status().state == "running",
        st.tg.status().state == "running",
        st.zapret.status().state == "running",
    );
    let mut restarted = Vec::new();
    if vpn_on {
        vpn::restart(&app).await.map_err(err_str)?;
        restarted.push("VPN".to_string());
    }
    if tg_on {
        tgproxy::stop(&app).await.map_err(err_str)?;
        tgproxy::start(&app).await.map_err(err_str)?;
        restarted.push("TgWsProxy".to_string());
    }
    if dpi_on {
        zapret::stop(&app).await.map_err(err_str)?;
        zapret::start(&app).await.map_err(err_str)?;
        restarted.push("Zapret".to_string());
    }
    Ok(restarted)
}
