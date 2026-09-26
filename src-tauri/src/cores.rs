//! Ядра: mihomo (VPN), tg-ws-proxy (Telegram), zapret (обход DPI).
//!
//! У каждого ядра своя папка `cores/<id>` с файлом `version`. Скачанная версия
//! лежит в данных приложения и имеет приоритет над той, что пришла с установщиком
//! (папку установки без прав администратора не обновить).

use crate::state::AppState;
use crate::util::{self, now_ms};
use crate::vpn;
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// Как часто проверять новые версии сами, без нажатия кнопки.
const CHECK_EVERY_MS: u64 = 24 * 3600 * 1000;

pub struct Spec {
    pub id: &'static str,
    pub title: &'static str,
    pub repo: &'static str,
    /// Главный исполняемый файл внутри папки ядра.
    pub exe: &'static str,
    asset: fn(tag: &str, name: &str) -> bool,
    /// Куда класть файл из архива: путь внутри папки ядра или None — пропустить.
    place: fn(entry: &str) -> Option<String>,
}

pub const SPECS: &[Spec] = &[
    Spec {
        id: "mihomo",
        title: "mihomo",
        repo: "MetaCubeX/mihomo",
        exe: "mihomo.exe",
        // «compatible» работает на любом x86-64 процессоре, в том числе без AVX2.
        asset: |tag, name| name == format!("mihomo-windows-amd64-compatible-{tag}.zip"),
        place: |e| e.to_lowercase().ends_with(".exe").then(|| "mihomo.exe".into()),
    },
    Spec {
        id: "tgws",
        title: "tg-ws-proxy",
        repo: "valnesfjord/tg-ws-proxy-rs",
        exe: "tg-ws-proxy.exe",
        asset: |_, name| name == "tg-ws-proxy-x86_64-pc-windows-gnu.zip",
        place: |e| e.to_lowercase().ends_with(".exe").then(|| "tg-ws-proxy.exe".into()),
    },
    Spec {
        id: "zapret",
        title: "Zapret",
        // Набор Flowseal: готовые стратегии под российских провайдеров,
        // списки сайтов и все нужные файлы в одном архиве.
        repo: "Flowseal/zapret-discord-youtube",
        exe: "bin/winws.exe",
        asset: |tag, name| name == format!("zapret-discord-youtube-{tag}.zip"),
        place: |e| {
            // В архиве всё лежит в папке с версией — её отбрасываем.
            let rel = e.split_once('/').map_or(e, |(_, r)| r);
            if rel.starts_with("bin/") || rel.starts_with("lists/") {
                Some(rel.to_string())
            } else if rel.ends_with(".bat") && !rel.contains('/') && rel != "service.bat" {
                Some(rel.to_string())
            } else {
                None
            }
        },
    },
];

pub fn spec(id: &str) -> Result<&'static Spec> {
    SPECS.iter().find(|s| s.id == id).ok_or_else(|| anyhow!("неизвестное ядро {id}"))
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CoreInfo {
    pub id: String,
    pub title: String,
    pub repo: String,
    pub installed: bool,
    pub version: Option<String>,
    /// Последняя версия на GitHub (из последней проверки).
    pub latest: Option<String>,
    pub update_available: bool,
    pub bundled: bool,
}

fn downloaded_dir(app: &AppHandle, id: &str) -> PathBuf {
    app.state::<AppState>().paths.cores.join(id)
}

fn bundled_dir(app: &AppHandle, id: &str) -> Option<PathBuf> {
    let res = app.state::<AppState>().paths.resources.clone()?;
    Some(res.join("cores").join(id))
}

/// Папка, из которой ядро реально запускается.
fn active_dir(app: &AppHandle, s: &Spec) -> Option<(PathBuf, bool)> {
    let own = downloaded_dir(app, s.id);
    if own.join(s.exe).exists() {
        return Some((own, false));
    }
    bundled_dir(app, s.id).filter(|d| d.join(s.exe).exists()).map(|d| (d, true))
}

pub fn info(app: &AppHandle) -> Vec<CoreInfo> {
    let latest = app.state::<AppState>().settings.lock().unwrap().core_latest.clone();
    SPECS
        .iter()
        .map(|s| {
            let dir = active_dir(app, s);
            let version = dir
                .as_ref()
                .and_then(|(d, _)| std::fs::read_to_string(d.join("version")).ok())
                .map(|v| v.trim().to_string());
            let latest = latest.get(s.id).cloned();
            CoreInfo {
                id: s.id.into(),
                title: s.title.into(),
                repo: s.repo.into(),
                installed: dir.is_some(),
                update_available: matches!((&version, &latest), (Some(v), Some(l)) if v != l),
                bundled: dir.as_ref().is_some_and(|(_, b)| *b),
                version,
                latest,
            }
        })
        .collect()
}

/// Путь к исполняемому файлу ядра; если ядра нет — скачивает последнюю версию.
pub async fn ensure(app: &AppHandle, id: &str) -> Result<PathBuf> {
    let s = spec(id)?;
    if let Some((dir, _)) = active_dir(app, s) {
        return Ok(dir.join(s.exe));
    }
    install(app, s).await?;
    Ok(downloaded_dir(app, s.id).join(s.exe))
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

/// Клиент для GitHub: напрямую, а если не вышло и VPN включён — через него.
async fn get(app: &AppHandle, url: &str, timeout: Duration) -> Result<reqwest::Response> {
    let direct = util::direct_client(timeout)?;
    match direct.get(url).send().await.and_then(|r| r.error_for_status()) {
        Ok(r) => Ok(r),
        Err(e) => match app.state::<AppState>().vpn.mixed_port() {
            Some(port) => Ok(util::proxied_client(port, timeout)?.get(url).send().await?.error_for_status()?),
            None => Err(e.into()),
        },
    }
}

async fn latest_release(app: &AppHandle, s: &Spec) -> Result<Release> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", s.repo);
    get(app, &url, Duration::from_secs(20))
        .await?
        .json()
        .await
        .with_context(|| format!("не удалось получить релизы {}", s.repo))
}

async fn install(app: &AppHandle, s: &Spec) -> Result<String> {
    let st = app.state::<AppState>();
    let rel = latest_release(app, s).await?;
    let asset = rel
        .assets
        .iter()
        .find(|a| (s.asset)(&rel.tag_name, &a.name))
        .ok_or_else(|| anyhow!("в релизе {} {} нет сборки для Windows", s.title, rel.tag_name))?;
    st.logs.push(app, "app", "info", format!("Скачиваю {} {}…", s.title, rel.tag_name));
    let bytes = get(app, &asset.browser_download_url, Duration::from_secs(300)).await?.bytes().await?;

    // Распаковываем во временную папку и подменяем целиком: ядро не останется полуобновлённым.
    let dir = downloaded_dir(app, s.id);
    let tmp = dir.with_extension("new");
    let _ = std::fs::remove_dir_all(&tmp);
    unpack(&bytes, &tmp, s.place)?;
    if !tmp.join(s.exe).exists() {
        let _ = std::fs::remove_dir_all(&tmp);
        bail!("в архиве {} нет {}", asset.name, s.exe);
    }
    std::fs::write(tmp.join("version"), &rel.tag_name)?;
    let old = dir.with_extension("old");
    let _ = std::fs::remove_dir_all(&old);
    if dir.exists() {
        std::fs::rename(&dir, &old).context("файлы ядра заняты — оно ещё работает?")?;
    }
    std::fs::rename(&tmp, &dir)?;
    let _ = std::fs::remove_dir_all(&old);

    st.update_settings(|set| {
        set.core_latest.insert(s.id.into(), rel.tag_name.clone());
    })?;
    st.logs.push(app, "app", "info", format!("{} {} установлен", s.title, rel.tag_name));
    Ok(rel.tag_name)
}

fn unpack(bytes: &[u8], dest: &Path, place: fn(&str) -> Option<String>) -> Result<()> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i)?;
        if f.is_dir() {
            continue;
        }
        let Some(rel) = place(f.name()) else { continue };
        let out = dest.join(rel);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut buf = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut buf)?;
        std::fs::write(out, buf)?;
    }
    Ok(())
}

/// Сверяет установленные версии с последними релизами на GitHub.
pub async fn check_updates(app: &AppHandle) -> Result<Vec<CoreInfo>> {
    let results = futures::future::join_all(SPECS.iter().map(|s| async move { (s.id, latest_release(app, s).await) })).await;
    let mut errors = Vec::new();
    let st = app.state::<AppState>();
    st.update_settings(|set| {
        for (id, r) in &results {
            match r {
                Ok(rel) => {
                    set.core_latest.insert((*id).into(), rel.tag_name.clone());
                }
                Err(e) => errors.push(format!("{id}: {e:#}")),
            }
        }
        set.core_checked_at = now_ms();
    })?;
    if errors.len() == results.len() {
        bail!("GitHub недоступен: {}", errors.join("; "));
    }
    let list = info(app);
    let _ = app.emit("cores", &list);
    Ok(list)
}

/// Проверка раз в сутки в фоне — чтобы о новой версии узнавать без захода в настройки.
pub fn schedule_checks(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(20)).await;
        loop {
            let last = app.state::<AppState>().settings.lock().unwrap().core_checked_at;
            if now_ms().saturating_sub(last) >= CHECK_EVERY_MS {
                let _ = check_updates(&app).await;
            }
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    });
}

/// Обновляет ядро до последней версии. Если оно сейчас работает — останавливает
/// и после обновления запускает снова.
pub async fn update(app: &AppHandle, id: &str) -> Result<String> {
    let s = spec(id)?;
    let was_running = match id {
        "mihomo" => {
            let running = app.state::<AppState>().vpn.status().state == "running";
            if running {
                vpn::stop(app).await?;
            }
            running
        }
        _ => false, // tg-ws-proxy и zapret подключатся здесь же, когда появятся их разделы
    };
    let result = install(app, s).await;
    if was_running {
        vpn::start(app).await?;
    }
    let _ = app.emit("cores", &info(app));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Скачивает последние релизы всех ядер и распаковывает их по правилам Spec:
    /// `cargo test --lib real_releases_unpack -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_releases_unpack() {
        tauri::async_runtime::block_on(async {
            let client = util::direct_client(Duration::from_secs(300)).unwrap();
            for s in SPECS {
                let rel: Release = client
                    .get(format!("https://api.github.com/repos/{}/releases/latest", s.repo))
                    .send().await.unwrap().json().await.unwrap();
                let asset = rel.assets.iter().find(|a| (s.asset)(&rel.tag_name, &a.name))
                    .unwrap_or_else(|| panic!("{}: нет подходящего архива в {}", s.id, rel.tag_name));
                let bytes = client.get(&asset.browser_download_url).send().await.unwrap().bytes().await.unwrap();
                let dir = std::env::temp_dir().join(format!("kkm-core-{}", s.id));
                let _ = std::fs::remove_dir_all(&dir);
                unpack(&bytes, &dir, s.place).unwrap();
                let files: Vec<String> = walk(&dir);
                println!("{} {} -> {:?}", s.id, rel.tag_name, files);
                assert!(dir.join(s.exe).exists(), "{}: нет {}", s.id, s.exe);
            }
        });
    }

    fn walk(dir: &Path) -> Vec<String> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            if e.path().is_dir() {
                let n = std::fs::read_dir(e.path()).unwrap().count();
                out.push(format!("{}/ ({n} файлов)", e.file_name().to_string_lossy()));
            } else {
                out.push(e.file_name().to_string_lossy().into_owned());
            }
        }
        out
    }
}

/// Папка установленного ядра — чтобы другие модули могли читать его файлы.
pub fn installed_dir(app: &AppHandle, id: &str) -> Option<PathBuf> {
    let s = spec(id).ok()?;
    active_dir(app, s).map(|(dir, _)| dir)
}
