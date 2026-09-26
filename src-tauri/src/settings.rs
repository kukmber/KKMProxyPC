use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub active_profile: Option<String>,
    /// rule | global | direct
    pub mode: String,
    /// sysproxy | tun
    pub connection: String,
    /// Предпочитаемый порт HTTP/SOCKS; если занят — берётся свободный.
    pub mixed_port: u16,
    /// Порт, на который мы направили системный прокси. Нужен, чтобы после
    /// аварийного завершения не оставить Windows с прокси на мёртвый порт.
    pub sysproxy_port: Option<u16>,
    /// Последние версии ядер на GitHub по итогам проверки.
    /// Адрес, порт и постоянный секрет прокси для Telegram.
    pub tg_host: String,
    pub tg_port: u16,
    pub tg_secret: Option<String>,
    /// Выбранная стратегия обхода DPI: id встроенной или "custom".
    pub zapret_strategy: String,
    /// Своя стратегия — аргументы winws как есть.
    pub zapret_custom: String,
    /// Применять обход только к доменам из списка.
    pub zapret_hostlist_on: bool,
    /// Отмеченные готовые наборы доменов и свои адреса.
    pub zapret_sets: Vec<String>,
    pub zapret_custom_hosts: String,
    /// Что включать сразу после запуска программы.
    pub autostart_vpn: bool,
    pub autostart_tg: bool,
    pub autostart_zapret: bool,
    /// Прятать окно в трей вместо выхода при нажатии на крестик.
    pub tray_enabled: bool,
    /// Запускать свёрнутым, когда программа стартует вместе с Windows.
    pub start_minimized: bool,
    pub core_latest: HashMap<String, String>,
    pub core_checked_at: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            active_profile: None,
            mode: "rule".into(),
            connection: "sysproxy".into(),
            mixed_port: 7890,
            sysproxy_port: None,
            tg_host: "127.0.0.1".into(),
            tg_port: 1443,
            tg_secret: None,
            zapret_strategy: "general".into(),
            zapret_custom: String::new(),
            zapret_hostlist_on: false,
            zapret_sets: Vec::new(),
            zapret_custom_hosts: String::new(),
            autostart_vpn: false,
            autostart_tg: false,
            autostart_zapret: false,
            tray_enabled: true,
            start_minimized: true,
            core_latest: HashMap::new(),
            core_checked_at: 0,
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        crate::util::write_atomic(path, serde_json::to_vec_pretty(self)?)
    }
}
