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
use serde::{Deserialize, Serialize};
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

/// Смена адреса, порта или ключа перезапускает прокси — старая ссылка
/// перестаёт действовать, поэтому её показываем заново.
#[tauri::command]
pub async fn set_tg_params(
    app: AppHandle,
    st: State<'_, AppState>,
    host: String,
    port: u16,
    secret: String,
) -> R<()> {
    let host = host.trim().to_string();
    if host.is_empty() {
        return Err("Впишите адрес, например 127.0.0.1".into());
    }
    if port == 0 {
        return Err("Порт должен быть от 1 до 65535".into());
    }
    let secret = secret.trim().to_ascii_lowercase();
    if !tgproxy::valid_secret(&secret) {
        return Err("Секретный ключ — ровно 32 знака из цифр и букв a–f".into());
    }
    st.update_settings(|s| {
        s.tg_host = host;
        s.tg_port = port;
        s.tg_secret = Some(secret);
    })
    .map_err(err_str)?;
    if st.tg.status().state == "running" {
        tgproxy::stop(&app).await.map_err(err_str)?;
        tgproxy::start(&app).await.map_err(err_str)?;
    }
    Ok(())
}

/// Выдаёт новый случайный ключ: пригодится, если прежний куда-то утёк.
#[tauri::command]
pub async fn regenerate_tg_secret(app: AppHandle, st: State<'_, AppState>) -> R<String> {
    let secret = tgproxy::regenerate_secret(&app).map_err(err_str)?;
    if st.tg.status().state == "running" {
        tgproxy::stop(&app).await.map_err(err_str)?;
        tgproxy::start(&app).await.map_err(err_str)?;
    }
    Ok(secret)
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

#[tauri::command]
pub fn zapret_strategies(app: AppHandle) -> Vec<zapret::StrategyInfo> {
    zapret::list_strategies(&app)
}

#[tauri::command]
pub async fn set_zapret_strategy(app: AppHandle, st: State<'_, AppState>, id: String) -> R<()> {
    // Проверяем параметры до запуска: winws сам разберёт их и выйдет.
    let args = zapret::args_of(&app, &id).map_err(err_str)?;
    zapret::check(&app, &args).await.map_err(err_str)?;
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostsState {
    pub sets: Vec<String>,
    pub custom: String,
    pub total: usize,
    pub available: Vec<crate::hostsets::HostSetInfo>,
}

#[tauri::command]
pub fn zapret_hosts(st: State<AppState>) -> HostsState {
    let s = st.settings.lock().unwrap();
    HostsState {
        total: crate::hostsets::collect(&s.zapret_sets, &s.zapret_custom_hosts).len(),
        sets: s.zapret_sets.clone(),
        custom: s.zapret_custom_hosts.clone(),
        available: crate::hostsets::list(),
    }
}

#[tauri::command]
pub async fn set_zapret_hosts(
    app: AppHandle,
    st: State<'_, AppState>,
    sets: Vec<String>,
    custom: String,
) -> R<usize> {
    st.update_settings(|s| {
        s.zapret_sets = sets;
        s.zapret_custom_hosts = custom;
    })
    .map_err(err_str)?;
    let total = zapret::rebuild_hostlist(&app).map_err(err_str)?;
    if st.zapret.status().state == "running" {
        zapret::stop(&app).await.map_err(err_str)?;
        zapret::start(&app).await.map_err(err_str)?;
    }
    Ok(total)
}

/// Сохраняет свою стратегию, предварительно проверив её параметры.
#[tauri::command]
pub async fn set_zapret_custom(app: AppHandle, st: State<'_, AppState>, args: String) -> R<()> {
    if !args.trim().is_empty() {
        zapret::check(&app, &zapret::split_args(&args)).await.map_err(err_str)?;
    }
    st.update_settings(|s| s.zapret_custom = args).map_err(err_str)?;
    if st.zapret.status().state == "running"
        && st.settings.lock().unwrap().zapret_strategy == zapret::CUSTOM
    {
        zapret::stop(&app).await.map_err(err_str)?;
        zapret::start(&app).await.map_err(err_str)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn zapret_autotune(app: AppHandle, domains: Vec<String>) -> R<Vec<zapret::ProbeResult>> {
    zapret::autotune(&app, domains).await.map_err(err_str)
}

// ---------- запуск вместе с Windows ----------

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Startup {
    pub with_windows: bool,
    /// Как именно сделан автозапуск: задачей планировщика (с правами) или
    /// записью в реестре (без прав). Задаётся программой, из окна не меняется.
    #[serde(default)]
    pub elevated: bool,
    pub minimized: bool,
    pub vpn: bool,
    pub tg: bool,
    pub zapret: bool,
    pub tray: bool,
}

#[tauri::command]
pub fn get_startup(app: AppHandle, st: State<AppState>) -> Startup {
    let kind = crate::autostart::status(&app);
    let s = st.settings.lock().unwrap();
    Startup {
        with_windows: kind != crate::autostart::Kind::Off,
        elevated: kind == crate::autostart::Kind::Task,
        minimized: s.start_minimized,
        vpn: s.autostart_vpn,
        tg: s.autostart_tg,
        zapret: s.autostart_zapret,
        tray: s.tray_enabled,
    }
}

#[tauri::command]
pub fn set_startup(app: AppHandle, st: State<AppState>, value: Startup) -> R<()> {
    let was = crate::autostart::status(&app) != crate::autostart::Kind::Off;
    if value.with_windows != was {
        if value.with_windows {
            crate::autostart::enable(&app).map_err(err_str)?;
        } else {
            crate::autostart::disable(&app).map_err(err_str)?;
        }
    }
    st.update_settings(|s| {
        s.start_minimized = value.minimized;
        s.autostart_vpn = value.vpn;
        s.autostart_tg = value.tg;
        s.autostart_zapret = value.zapret;
        s.tray_enabled = value.tray;
    })
    .map_err(err_str)
}

/// Чужая программа обхода DPI, если она сейчас работает. Два обхода
/// одновременно перехватывают одни и те же пакеты и рвут сеть, поэтому
/// о таком соседстве предупреждаем заранее, а не по факту поломки.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DpiConflict {
    pub pid: u32,
    pub path: String,
}

#[tauri::command]
pub fn dpi_conflict(st: State<AppState>) -> Option<DpiConflict> {
    // Свой запущенный обход конфликтом не считаем.
    if st.zapret.status().state == "running" {
        return None;
    }
    winsys::find_process("winws.exe").map(|(pid, path)| DpiConflict { pid, path })
}

// ---------- правила по программам ----------

#[tauri::command]
pub fn app_rules(st: State<AppState>) -> Vec<crate::config::AppRule> {
    st.settings.lock().unwrap().app_rules.clone()
}

/// Сохраняет правила. Ядро перечитывает их только при перезапуске, поэтому
/// работающий VPN поднимаем заново.
#[tauri::command]
pub async fn set_app_rules(
    app: AppHandle,
    st: State<'_, AppState>,
    rules: Vec<crate::config::AppRule>,
) -> R<()> {
    st.update_settings(|s| s.app_rules = rules).map_err(err_str)?;
    if st.vpn.status().state == "running" {
        vpn::restart(&app).await.map_err(err_str)?;
    }
    Ok(())
}

/// Запущенные программы — список для выбора при добавлении правила.
#[tauri::command]
pub fn running_processes() -> Vec<String> {
    winsys::running_processes()
}

// ---------- горячие клавиши ----------

#[tauri::command]
pub fn hotkeys(app: AppHandle) -> Vec<crate::hotkeys::HotkeyInfo> {
    crate::hotkeys::list(&app)
}

/// Пустое сочетание убирает горячую клавишу.
#[tauri::command]
pub fn set_hotkey(app: AppHandle, action: String, accelerator: String) -> R<()> {
    crate::hotkeys::set(&app, &action, &accelerator).map_err(err_str)
}
