//! Кэширующие серверы YouTube в России (GGC).
//!
//! С ноября 2026 российские кэши YouTube отдают видео очень медленно или не
//! отдают вовсе — это признаёт и автор набора Flowseal. Полностью лечится
//! только VPN, но частично помогает закрыть неработающие кэши в файле
//! `hosts`: плеер тогда выбирает зарубежные серверы.
//!
//! Здесь это делается автоматически: берём страницу YouTube, вытаскиваем из
//! неё имена кэшей, узнаём их страну и в `hosts` пишем только российские.
//! Проверять нужно при выключенном VPN — иначе видны кэши страны VPN.

use crate::state::AppState;
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use tauri::{AppHandle, Manager};

/// Маркеры своего блока в hosts: по ним же его потом убираем.
const MARK_BEGIN: &str = "# KKMProxy: YouTube RU caches (auto)";
const MARK_END: &str = "# /KKMProxy: YouTube RU caches";

const WATCH_URL: &str = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) \
                   Chrome/131.0.0.0 Safari/537.36";

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CacheHost {
    pub host: String,
    /// Адрес, который вернул DNS. Пусто — не удалось узнать.
    pub ip: String,
    /// Код страны по GeoIP. Пусто — не удалось узнать.
    pub country: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct YoutubeReport {
    pub hosts: Vec<CacheHost>,
    /// Сколько кэшей оказались российскими.
    pub ru: usize,
    /// Сколько записей уже лежит в hosts.
    pub blocked: usize,
    /// Удалось ли определить страну (без этого российские не отличить).
    pub geo_ok: bool,
    /// Включён ли VPN: при нём проверка показывает кэши страны выхода.
    pub vpn_running: bool,
}

fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(20))
        .build()
        .context("не удалось создать HTTP-клиент")
}

/// Все имена кэшей `rrN---sn-….googlevideo.com` со страницы YouTube.
pub fn extract_hosts(html: &str) -> Vec<String> {
    let bytes = html.as_bytes();
    let mut found = BTreeMap::new();
    for (idx, _) in html.match_indices("googlevideo.com") {
        let mut start = idx;
        while start > 0 {
            let c = bytes[start - 1];
            if c.is_ascii_alphanumeric() || c == b'-' || c == b'.' {
                start -= 1;
            } else {
                break;
            }
        }
        let host = &html[start..idx + "googlevideo.com".len()];
        if host.starts_with("rr") && host.contains("---sn-") && host.ends_with(".googlevideo.com") {
            found.insert(host.to_string(), ());
        }
    }
    found.into_keys().collect()
}

/// Адрес кэша по DNS-over-HTTPS: обычный DNS может отдавать подменённые
/// адреса (fake-ip от VPN), а страну нужно знать настоящую.
async fn resolve(client: &reqwest::Client, host: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Answer {
        #[serde(rename = "type")]
        kind: u32,
        data: String,
    }
    #[derive(Deserialize)]
    struct Resp {
        #[serde(default)]
        answer: Vec<Answer>,
    }
    let endpoints = [
        ("https://dns.google/resolve", true),
        ("https://cloudflare-dns.com/dns-query", false),
    ];
    for (url, google) in endpoints {
        let mut req = client.get(url).query(&[("name", host), ("type", "A")]);
        if !google {
            req = req.header("accept", "application/dns-json");
        }
        let Ok(resp) = req.send().await else { continue };
        let Ok(data) = resp.json::<Resp>().await else { continue };
        if let Some(a) = data.answer.into_iter().find(|a| a.kind == 1) {
            return Some(a.data);
        }
    }
    None
}

/// Страны адресов. Возвращает карту «адрес — код страны» и признак того,
/// что сервис вообще ответил.
///
/// Основной сервис — `ipwho.is`: работает по HTTPS и доступен там, где
/// `ip-api.com` блокируется провайдером (запросы к нему просто не проходят).
/// `ip-api.com` оставлен резервом: он умеет отдавать все адреса одним запросом.
async fn countries(client: &reqwest::Client, ips: &[String]) -> (BTreeMap<String, String>, bool) {
    if ips.is_empty() {
        return (BTreeMap::new(), true);
    }

    let answers = futures::future::join_all(ips.iter().map(|ip| whois_country(client, ip))).await;
    let mut map = BTreeMap::new();
    let mut answered = false;
    for (ip, answer) in ips.iter().zip(answers) {
        let Some(country) = answer else { continue };
        answered = true;
        if !country.is_empty() {
            map.insert(ip.clone(), country);
        }
    }
    if answered {
        return (map, true);
    }

    ipapi_countries(client, ips).await
}

/// Страна одного адреса по `ipwho.is`. `None` — сервис не ответил,
/// `Some("")` — ответил, но страну не знает (служебные адреса и т. п.).
async fn whois_country(client: &reqwest::Client, ip: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Who {
        #[serde(default)]
        success: bool,
        #[serde(default, rename = "country_code")]
        country: String,
    }
    let resp = client.get(format!("https://ipwho.is/{ip}")).send().await.ok()?;
    let who = resp.json::<Who>().await.ok()?;
    Some(if who.success { who.country } else { String::new() })
}

/// Резервный сервис: один POST на все адреса.
async fn ipapi_countries(client: &reqwest::Client, ips: &[String]) -> (BTreeMap<String, String>, bool) {
    #[derive(Deserialize)]
    struct Geo {
        query: String,
        #[serde(rename = "countryCode", default)]
        country: String,
    }
    let resp = client
        .post("http://ip-api.com/batch?fields=status,countryCode,query")
        .json(ips)
        .send()
        .await;
    let Ok(resp) = resp else { return (BTreeMap::new(), false) };
    let Ok(list) = resp.json::<Vec<Geo>>().await else { return (BTreeMap::new(), false) };
    let map = list
        .into_iter()
        .filter(|g| !g.country.is_empty())
        .map(|g| (g.query, g.country))
        .collect::<BTreeMap<_, _>>();
    let ok = !map.is_empty();
    (map, ok)
}

/// Находит кэши, которыми сейчас пользуется YouTube, и определяет их страну.
pub async fn check(app: &AppHandle) -> Result<YoutubeReport> {
    let client = client()?;
    let html = client
        .get(WATCH_URL)
        .send()
        .await
        .context("YouTube не ответил — проверьте интернет")?
        .error_for_status()
        .context("YouTube вернул ошибку")?
        .text()
        .await
        .context("не удалось прочитать страницу YouTube")?;
    let hosts = extract_hosts(&html);
    if hosts.is_empty() {
        bail!("YouTube не вернул адреса кэшей — попробуйте ещё раз");
    }

    let resolved = futures::future::join_all(hosts.iter().map(|h| resolve(&client, h))).await;
    let mut list: Vec<CacheHost> = hosts
        .into_iter()
        .zip(resolved)
        .map(|(host, ip)| CacheHost { host, ip: ip.unwrap_or_default(), country: String::new() })
        .collect();

    let ips: Vec<String> = list.iter().map(|c| c.ip.clone()).filter(|ip| !ip.is_empty()).collect();
    let (geo, geo_ok) = countries(&client, &ips).await;
    for c in &mut list {
        c.country = geo.get(&c.ip).cloned().unwrap_or_default();
    }
    list.sort_by(|a, b| a.host.cmp(&b.host));

    let ru = list.iter().filter(|c| c.country == "RU").count();
    let bytes = std::fs::read(hosts_path()).unwrap_or_default();
    let vpn_running = app.state::<AppState>().vpn.status().state == "running";
    Ok(YoutubeReport { hosts: list, ru, blocked: blocked_hosts(&bytes).len(), geo_ok, vpn_running })
}

fn hosts_path() -> PathBuf {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    PathBuf::from(root).join(r"System32\drivers\etc\hosts")
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Убирает прежний блок целиком, не трогая остальные строки: файл hosts
/// может быть в ANSI, поэтому работаем с байтами, а не со строкой.
fn cut_block(bytes: &[u8]) -> Vec<u8> {
    let Some(begin) = find(bytes, MARK_BEGIN.as_bytes()) else { return bytes.to_vec() };
    let Some(end) = find(bytes, MARK_END.as_bytes()) else { return bytes.to_vec() };
    if end < begin {
        return bytes.to_vec();
    }
    let mut start = begin;
    while start > 0 && bytes[start - 1] != b'\n' {
        start -= 1;
    }
    let mut stop = end + MARK_END.len();
    while stop < bytes.len() && bytes[stop] != b'\n' {
        stop += 1;
    }
    if stop < bytes.len() {
        stop += 1;
    }
    [&bytes[..start], &bytes[stop..]].concat()
}

/// Готовое содержимое hosts: прежний блок заменяется новым.
pub fn with_block(bytes: &[u8], hosts: &[String]) -> Vec<u8> {
    let mut out = cut_block(bytes);
    while matches!(out.last(), Some(b'\n' | b'\r')) {
        out.pop();
    }
    if !out.is_empty() {
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(MARK_BEGIN.as_bytes());
    out.extend_from_slice(b"\r\n");
    for host in hosts {
        out.extend_from_slice(b"0.0.0.0 ");
        out.extend_from_slice(host.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(MARK_END.as_bytes());
    out.extend_from_slice(b"\r\n");
    out
}

/// Имена, которые уже закрыты нашим блоком.
pub fn blocked_hosts(bytes: &[u8]) -> Vec<String> {
    let Some(begin) = find(bytes, MARK_BEGIN.as_bytes()) else { return Vec::new() };
    let Some(end) = find(bytes, MARK_END.as_bytes()) else { return Vec::new() };
    if end < begin {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&bytes[begin..end]);
    text.lines()
        .filter_map(|line| line.trim().strip_prefix("0.0.0.0 "))
        .map(|host| host.trim().to_string())
        .filter(|host| !host.is_empty())
        .collect()
}

/// Закрывает перечисленные кэши в hosts. Перед первой правкой кладёт копию
/// файла рядом с настройками программы.
pub fn block(app: &AppHandle, hosts: &[String]) -> Result<usize> {
    let hosts: Vec<String> = hosts
        .iter()
        .map(|h| h.trim().to_ascii_lowercase())
        .filter(|h| {
            h.starts_with("rr")
                && h.ends_with(".googlevideo.com")
                && h.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
        })
        .collect();
    if hosts.is_empty() {
        bail!("Не выбрано ни одного кэша");
    }
    let path = hosts_path();
    let bytes = std::fs::read(&path).context("не удалось прочитать файл hosts")?;
    let backup = app.state::<AppState>().paths.root.join("hosts.backup");
    if !backup.exists() {
        let _ = std::fs::write(&backup, &bytes);
    }
    // Новые кэши добавляем к уже закрытым: список у провайдера меняется,
    // и повторная проверка не должна разблокировать прежние.
    let mut all = blocked_hosts(&bytes);
    for host in hosts {
        if !all.contains(&host) {
            all.push(host);
        }
    }
    all.sort();
    std::fs::write(&path, with_block(&bytes, &all)).map_err(|e| {
        anyhow!("не удалось записать hosts ({e}) — запустите программу от администратора")
    })?;
    flush_dns();
    Ok(all.len())
}

/// Убирает наш блок из hosts. `true` — если он там был.
pub fn unblock(_app: &AppHandle) -> Result<bool> {
    let path = hosts_path();
    let bytes = std::fs::read(&path).context("не удалось прочитать файл hosts")?;
    if find(&bytes, MARK_BEGIN.as_bytes()).is_none() {
        return Ok(false);
    }
    std::fs::write(&path, cut_block(&bytes))
        .map_err(|e| anyhow!("не удалось записать hosts ({e}) — запустите программу от администратора"))?;
    flush_dns();
    Ok(true)
}

/// Windows кэширует ответы DNS, поэтому без сброса новые записи подхватятся
/// не сразу. Ошибку игнорируем: это лишь ускоряет применение. Флаг
/// CREATE_NO_WINDOW — чтобы у программы не мигало чёрное окно консоли.
fn flush_dns() {
    use std::os::windows::process::CommandExt;
    let _ = Command::new("ipconfig")
        .arg("/flushdns")
        .creation_flags(0x0800_0000)
        .output();
}

/// Сколько записей уже закрыто в hosts.
pub fn blocked_count() -> usize {
    blocked_hosts(&std::fs::read(hosts_path()).unwrap_or_default()).len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_only_cache_hosts() {
        let html = r#"preload('https:\/\/rr3---sn-2oi5h5-5j.googlevideo.com\/generate_204');
            "url":"https:\/\/rr1---sn-xguxaxjvh-8v1e.googlevideo.com\/videoplayback?x=1"
            https://example.com/googlevideo.com/ https://notgooglevideo.com/x"#;
        let hosts = extract_hosts(html);
        assert_eq!(
            hosts,
            vec![
                "rr1---sn-xguxaxjvh-8v1e.googlevideo.com".to_string(),
                "rr3---sn-2oi5h5-5j.googlevideo.com".to_string(),
            ]
        );
    }

    #[test]
    fn block_replaces_itself_and_keeps_foreign_lines() {
        let original = b"127.0.0.1 localhost\r\n# my comment\r\n10.0.0.1 router\r\n";
        let first = with_block(original, &["rr1---sn-a.googlevideo.com".into()]);
        let text = String::from_utf8(first.clone()).unwrap();
        assert!(text.contains("127.0.0.1 localhost"));
        assert!(text.contains("10.0.0.1 router"));
        assert!(text.contains("0.0.0.0 rr1---sn-a.googlevideo.com"));

        let second = with_block(&first, &["rr2---sn-b.googlevideo.com".into()]);
        let text = String::from_utf8(second.clone()).unwrap();
        assert_eq!(text.matches(MARK_BEGIN).count(), 1, "блок не должен дублироваться");
        assert!(!text.contains("rr1---sn-a.googlevideo.com"));
        assert!(text.contains("0.0.0.0 rr2---sn-b.googlevideo.com"));
        assert_eq!(blocked_hosts(&second), vec!["rr2---sn-b.googlevideo.com".to_string()]);

        let clean = String::from_utf8(cut_block(&second)).unwrap();
        assert!(!clean.contains(MARK_BEGIN));
        assert!(clean.contains("# my comment"));
        assert!(clean.contains("10.0.0.1 router"));
    }
}
