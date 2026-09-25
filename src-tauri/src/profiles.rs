use crate::state::AppState;
use crate::subscription::{self, UserInfo};
use crate::util::{self, now_ms};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::{AppHandle, Manager};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub url: Option<String>,
    /// yaml | links
    pub kind: String,
    pub updated_at: u64,
    pub userinfo: Option<UserInfo>,
    pub proxy_count: usize,
}

pub fn load(path: &std::path::Path) -> Vec<Profile> {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save(st: &AppState, list: &[Profile]) -> Result<()> {
    util::write_atomic(&st.paths.profiles_file(), serde_json::to_vec_pretty(list)?)
}

/// Скачивает подписку напрямую; если не вышло и VPN включён — пробует через него.
async fn fetch(app: &AppHandle, url: &str) -> Result<subscription::Fetched> {
    let direct = util::direct_client(Duration::from_secs(20))?;
    match subscription::fetch(&direct, url).await {
        Ok(f) => Ok(f),
        Err(e) => {
            let st = app.state::<AppState>();
            match st.vpn.mixed_port() {
                Some(port) => {
                    let via = util::proxied_client(port, Duration::from_secs(30))?;
                    subscription::fetch(&via, url).await
                }
                None => Err(e),
            }
        }
    }
}

fn host_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_else(|| "Подписка".into())
}

pub async fn add_from_url(app: &AppHandle, url: &str) -> Result<Profile> {
    let url = url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        // Одиночная ссылка vless:// и т.п. — это не подписка, а сам сервер.
        return add_from_content(app, None, url);
    }
    let f = fetch(app, url).await?;
    let parsed = subscription::parse(&f.body)?;
    let st = app.state::<AppState>();
    let id = util::random_hex(6);
    util::write_atomic(&st.paths.profile_content(&id), &parsed.yaml)?;
    let p = Profile {
        id,
        name: f.title.unwrap_or_else(|| host_of(url)),
        url: Some(url.to_string()),
        kind: parsed.kind.into(),
        updated_at: now_ms(),
        userinfo: f.userinfo,
        proxy_count: parsed.proxy_count,
    };
    insert(app, p.clone())?;
    Ok(p)
}

pub fn add_from_content(app: &AppHandle, name: Option<String>, content: &str) -> Result<Profile> {
    let parsed = subscription::parse(content)?;
    let st = app.state::<AppState>();
    let id = util::random_hex(6);
    util::write_atomic(&st.paths.profile_content(&id), &parsed.yaml)?;
    let p = Profile {
        id,
        name: name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "Мой профиль".into()),
        url: None,
        kind: parsed.kind.into(),
        updated_at: now_ms(),
        userinfo: None,
        proxy_count: parsed.proxy_count,
    };
    insert(app, p.clone())?;
    Ok(p)
}

fn insert(app: &AppHandle, p: Profile) -> Result<()> {
    let st = app.state::<AppState>();
    let mut list = st.profiles.lock().unwrap();
    list.push(p.clone());
    save(&st, &list)?;
    drop(list);
    st.update_settings(|s| {
        if s.active_profile.is_none() {
            s.active_profile = Some(p.id.clone());
        }
    })
}

pub async fn refresh(app: &AppHandle, id: &str) -> Result<Profile> {
    let st = app.state::<AppState>();
    let url = {
        let list = st.profiles.lock().unwrap();
        let p = list.iter().find(|p| p.id == id).ok_or_else(|| anyhow!("Профиль не найден"))?;
        p.url.clone().ok_or_else(|| anyhow!("У профиля нет ссылки для обновления"))?
    };
    let f = fetch(app, &url).await?;
    let parsed = subscription::parse(&f.body)?;
    util::write_atomic(&st.paths.profile_content(id), &parsed.yaml)?;
    let mut list = st.profiles.lock().unwrap();
    let p = list.iter_mut().find(|p| p.id == id).ok_or_else(|| anyhow!("Профиль не найден"))?;
    if let Some(t) = f.title {
        p.name = t;
    }
    if f.userinfo.is_some() {
        p.userinfo = f.userinfo;
    }
    p.kind = parsed.kind.into();
    p.proxy_count = parsed.proxy_count;
    p.updated_at = now_ms();
    let out = p.clone();
    save(&st, &list)?;
    Ok(out)
}

pub fn delete(app: &AppHandle, id: &str) -> Result<()> {
    let st = app.state::<AppState>();
    let mut list = st.profiles.lock().unwrap();
    list.retain(|p| p.id != id);
    save(&st, &list)?;
    let next = list.first().map(|p| p.id.clone());
    drop(list);
    let _ = std::fs::remove_file(st.paths.profile_content(id));
    st.update_settings(|s| {
        if s.active_profile.as_deref() == Some(id) {
            s.active_profile = next;
        }
    })
}

pub fn rename(app: &AppHandle, id: &str, name: &str) -> Result<()> {
    let st = app.state::<AppState>();
    let mut list = st.profiles.lock().unwrap();
    if let Some(p) = list.iter_mut().find(|p| p.id == id) {
        p.name = name.trim().to_string();
    }
    save(&st, &list)
}
