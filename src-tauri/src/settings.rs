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
    /// Тонкие настройки ядра Telegram — свои домены Cloudflare, Worker, DC→IP.
    pub tg_advanced: TgAdvanced,
    /// Выбранная стратегия обхода DPI: id встроенной или "custom".
    pub zapret_strategy: String,
    /// Своя стратегия — аргументы winws как есть.
    pub zapret_custom: String,
    /// Применять обход только к доменам из списка.
    pub zapret_hostlist_on: bool,
    /// Отмеченные готовые наборы доменов и свои адреса.
    pub zapret_sets: Vec<String>,
    pub zapret_custom_hosts: String,
    /// Горячие клавиши: действие — сочетание.
    pub hotkeys: HashMap<String, String>,
    /// Правила «эта программа — так»: применяются перед правилами подписки.
    pub app_rules: Vec<crate::config::AppRule>,
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
            tg_advanced: TgAdvanced::default(),
            zapret_strategy: "general".into(),
            zapret_custom: String::new(),
            zapret_hostlist_on: false,
            zapret_sets: Vec::new(),
            zapret_custom_hosts: String::new(),
            hotkeys: HashMap::new(),
            app_rules: Vec::new(),
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

/// Продвинутые настройки прокси для Telegram.
///
/// Повторяют то, что умеет само ядро tg-ws-proxy: свои домены Cloudflare
/// (обход блокировки адресов Telegram), Cloudflare Worker, ручные адреса DC
/// и прочие тонкости. Обычному пользователю они не нужны — блок в интерфейсе
/// так и подписан.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct TgAdvanced {
    /// Свои домены Cloudflare с записями kws1–kws5.
    pub cf_proxy: bool,
    pub cf_domains: String,
    /// Cloudflare Worker — туннель до DC без своей зоны в Cloudflare.
    pub cf_worker: bool,
    pub cf_worker_domains: String,
    /// Раскладывать соединения по доменам Cloudflare по кругу.
    pub cf_balance: bool,
    /// Публичный список доменов из репозитория Flowseal.
    pub default_domains: bool,
    /// Не гнать медиа через HTTP/2 — только WebSocket.
    pub no_h2: bool,
    /// Ходить в Cloudflare по ws:// (порт 80) вместо wss://.
    pub cf_disable_tls: bool,
    /// Подробный журнал ядра.
    pub verbose: bool,
    /// Размер буфера сокета, КиБ. 0 — как в ядре.
    pub buf_kb: u32,
    /// Сколько соединений держать прогретыми на каждый DC. 0 — как в ядре.
    pub pool_size: u32,
    /// Свои адреса DC: по строке вида `2:149.154.167.220`.
    pub dc_ips: String,
    /// Сколько мегабайт журнала держать в памяти.
    pub log_mb: u32,
}

impl Default for TgAdvanced {
    fn default() -> Self {
        Self {
            cf_proxy: false,
            cf_domains: String::new(),
            cf_worker: false,
            cf_worker_domains: String::new(),
            cf_balance: false,
            // Публичный список включён всегда — так прокси работает сразу
            // после установки, без своей зоны в Cloudflare.
            default_domains: true,
            no_h2: false,
            cf_disable_tls: false,
            verbose: false,
            buf_kb: 0,
            pool_size: 0,
            dc_ips: String::new(),
            log_mb: 1,
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
