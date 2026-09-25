use crate::logs::Logs;
use crate::paths::Paths;
use crate::profiles::{self, Profile};
use crate::settings::Settings;
use crate::tgproxy::TgManager;
use crate::vpn::VpnManager;
use crate::zapret::ZapretManager;
use std::sync::Mutex;

pub struct AppState {
    pub paths: Paths,
    pub settings: Mutex<Settings>,
    pub profiles: Mutex<Vec<Profile>>,
    pub logs: Logs,
    pub vpn: VpnManager,
    pub tg: TgManager,
    pub zapret: ZapretManager,
}

impl AppState {
    pub fn new(paths: Paths) -> Self {
        let settings = Settings::load(&paths.settings_file());
        let profiles = profiles::load(&paths.profiles_file());
        Self {
            paths,
            settings: Mutex::new(settings),
            profiles: Mutex::new(profiles),
            logs: Logs::default(),
            vpn: VpnManager::new(),
            tg: TgManager::new(),
            zapret: ZapretManager::new(),
        }
    }

    pub fn update_settings(&self, f: impl FnOnce(&mut Settings)) -> anyhow::Result<()> {
        let mut s = self.settings.lock().unwrap();
        f(&mut s);
        s.save(&self.paths.settings_file())
    }
}
