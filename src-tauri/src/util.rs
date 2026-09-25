use anyhow::{bail, Context, Result};
use std::net::TcpListener;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// User-Agent для подписок и файлов правил: панели по нему решают, в каком
/// формате отдавать подписку, и с `clash.meta` отдают полноценный YAML.
pub const UA: &str = "clash.meta/v1.19.14 (KKMProxyPC)";

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn random_hex(bytes: usize) -> String {
    (0..bytes).map(|_| format!("{:02x}", rand::random::<u8>())).collect()
}

pub fn free_port() -> Result<u16> {
    Ok(TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

pub fn port_is_free(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

pub fn write_atomic(path: &Path, data: impl AsRef<[u8]>) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp~");
    std::fs::write(&tmp, data).with_context(|| format!("запись {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("запись {}", path.display()))?;
    Ok(())
}

/// HTTP-клиент без системного прокси. Когда включён наш же системный прокси,
/// обычный клиент пошёл бы через ядро — а прямые загрузки нужны именно в обход него.
pub fn direct_client(timeout: Duration) -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .user_agent(UA)
        .connect_timeout(Duration::from_secs(6).min(timeout))
        .timeout(timeout)
        .build()?)
}

/// Клиент, который ходит через mixed-порт запущенного ядра.
pub fn proxied_client(mixed_port: u16, timeout: Duration) -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .proxy(reqwest::Proxy::all(format!("http://127.0.0.1:{mixed_port}"))?)
        .user_agent(UA)
        .timeout(timeout)
        .build()?)
}

pub async fn download_to(client: &reqwest::Client, url: &str, path: &Path) -> Result<()> {
    let resp = client.get(url).send().await?.error_for_status()?;
    let bytes = resp.bytes().await?;
    if bytes.is_empty() {
        bail!("пустой ответ");
    }
    write_atomic(path, &bytes)
}

pub fn err_str(e: anyhow::Error) -> String {
    format!("{e:#}")
}
