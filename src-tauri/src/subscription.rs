//! Загрузка и разбор подписок: Clash/Mihomo YAML берётся как есть,
//! base64 и списки ссылок превращаются в конфиг с группами «PROXY» и «Авто».

use anyhow::{anyhow, bail, Result};
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value as J};
use std::collections::{HashMap, HashSet};

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct UserInfo {
    pub upload: u64,
    pub download: u64,
    pub total: u64,
    pub expire: Option<u64>,
}

pub struct Fetched {
    pub body: String,
    pub title: Option<String>,
    pub userinfo: Option<UserInfo>,
}

pub async fn fetch(client: &reqwest::Client, url: &str) -> Result<Fetched> {
    let resp = client.get(url).send().await?.error_for_status()?;
    let h = resp.headers();
    let header = |name: &str| h.get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
    let title = header("profile-title")
        .map(|t| decode_title(&t))
        .or_else(|| header("content-disposition").and_then(|v| filename_from_disposition(&v)));
    let userinfo = header("subscription-userinfo").map(|v| parse_userinfo(&v));
    let body = resp.text().await?;
    Ok(Fetched { body, title, userinfo })
}

fn decode_title(t: &str) -> String {
    if let Some(b) = t.strip_prefix("base64:") {
        if let Some(s) = decode_b64(b) {
            return s.trim().to_string();
        }
    }
    t.trim().to_string()
}

fn filename_from_disposition(v: &str) -> Option<String> {
    let name = if let Some((_, rest)) = v.split_once("filename*=") {
        let rest = rest.split(';').next()?.trim();
        let enc = rest.rsplit("''").next()?;
        percent_decode_str(enc).decode_utf8_lossy().into_owned()
    } else {
        let (_, rest) = v.split_once("filename=")?;
        rest.split(';').next()?.trim().trim_matches('"').to_string()
    };
    let name = name.trim_end_matches(".yaml").trim_end_matches(".yml").trim().to_string();
    (!name.is_empty()).then_some(name)
}

fn parse_userinfo(v: &str) -> UserInfo {
    let mut u = UserInfo::default();
    for part in v.split(';') {
        let Some((k, val)) = part.split_once('=') else { continue };
        let n = val.trim().parse::<f64>().unwrap_or(0.0) as u64;
        match k.trim() {
            "upload" => u.upload = n,
            "download" => u.download = n,
            "total" => u.total = n,
            "expire" if n > 0 => u.expire = Some(n),
            _ => {}
        }
    }
    u
}

pub struct Parsed {
    /// yaml — подписка в формате Clash, links — собрана нами из ссылок
    pub kind: &'static str,
    pub yaml: String,
    pub proxy_count: usize,
}

pub fn parse(body: &str) -> Result<Parsed> {
    let body = body.trim_start_matches('\u{feff}');
    if let Ok(serde_yaml::Value::Mapping(m)) = serde_yaml::from_str::<serde_yaml::Value>(body) {
        let proxies = m.get("proxies").and_then(|p| p.as_sequence()).map_or(0, |s| s.len());
        if proxies > 0 || m.contains_key("proxy-providers") {
            return Ok(Parsed { kind: "yaml", yaml: body.to_string(), proxy_count: proxies });
        }
    }

    let text = if body.contains("://") {
        body.to_string()
    } else {
        decode_b64(body).ok_or_else(|| anyhow!("Не удалось распознать формат подписки"))?
    };

    let mut proxies = Vec::new();
    let mut errors = 0;
    for line in text.lines().map(str::trim).filter(|l| l.contains("://")) {
        match parse_link(line) {
            Ok(p) => proxies.push(p),
            Err(_) => errors += 1,
        }
    }
    if proxies.is_empty() {
        if errors > 0 {
            bail!("Ни одну ссылку не удалось разобрать ({errors} шт.)");
        }
        bail!("В подписке нет серверов");
    }
    dedupe_names(&mut proxies);
    let count = proxies.len();
    let yaml = serde_yaml::to_string(&build_config(proxies))?;
    Ok(Parsed { kind: "links", yaml, proxy_count: count })
}

fn build_config(proxies: Vec<J>) -> J {
    let names: Vec<J> = proxies.iter().map(|p| p["name"].clone()).collect();
    let mut select = vec![json!("Авто")];
    select.extend(names.iter().cloned());
    json!({
        "proxies": proxies,
        "proxy-groups": [
            { "name": "PROXY", "type": "select", "proxies": select },
            {
                "name": "Авто", "type": "url-test", "proxies": names,
                "url": "https://www.gstatic.com/generate_204", "interval": 300, "tolerance": 50
            }
        ],
        "rules": [
            "IP-CIDR,10.0.0.0/8,DIRECT,no-resolve",
            "IP-CIDR,172.16.0.0/12,DIRECT,no-resolve",
            "IP-CIDR,192.168.0.0/16,DIRECT,no-resolve",
            "IP-CIDR,169.254.0.0/16,DIRECT,no-resolve",
            "IP-CIDR,127.0.0.0/8,DIRECT,no-resolve",
            "DOMAIN-SUFFIX,local,DIRECT",
            "MATCH,PROXY"
        ]
    })
}

fn dedupe_names(proxies: &mut [J]) {
    let mut seen = HashSet::new();
    for p in proxies.iter_mut() {
        let base = p["name"].as_str().unwrap_or("proxy").to_string();
        let mut name = base.clone();
        let mut i = 2;
        while !seen.insert(name.clone()) {
            name = format!("{base} {i}");
            i += 1;
        }
        p["name"] = J::String(name);
    }
}

pub fn decode_b64(s: &str) -> Option<String> {
    let clean: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    for engine in [&STANDARD, &STANDARD_NO_PAD, &URL_SAFE, &URL_SAFE_NO_PAD] {
        if let Ok(bytes) = engine.decode(&clean) {
            if let Ok(text) = String::from_utf8(bytes) {
                return Some(text);
            }
        }
    }
    None
}

// ---------- ссылки ----------

struct Obj(Map<String, J>);

impl Obj {
    fn new() -> Self {
        Obj(Map::new())
    }
    fn set(&mut self, k: &str, v: impl Into<J>) {
        self.0.insert(k.into(), v.into());
    }
    fn opt(&mut self, k: &str, v: Option<&String>) {
        if let Some(v) = v.filter(|v| !v.is_empty()) {
            self.0.insert(k.into(), J::String(v.clone()));
        }
    }
    fn done(self) -> J {
        J::Object(self.0)
    }
}

type Q = HashMap<String, String>;

fn pct(s: &str) -> String {
    percent_decode_str(s).decode_utf8_lossy().into_owned()
}

fn is_true(v: Option<&String>) -> bool {
    matches!(v.map(String::as_str), Some("1" | "true" | "True"))
}

struct Link {
    user: String,
    host: String,
    port: Option<u16>,
    q: Q,
    name: Option<String>,
}

fn split_link(link: &str) -> Result<Link> {
    let url = url::Url::parse(link)?;
    let host = url.host_str().ok_or_else(|| anyhow!("нет адреса"))?;
    let host = host.trim_start_matches('[').trim_end_matches(']').to_string();
    let mut user = pct(url.username());
    if let Some(pw) = url.password() {
        user = format!("{user}:{}", pct(pw));
    }
    let mut q = Q::new();
    for (k, v) in url.query_pairs() {
        q.entry(k.into_owned()).or_insert_with(|| v.into_owned());
    }
    let name = url.fragment().map(pct).filter(|n| !n.trim().is_empty());
    Ok(Link { user, host, port: url.port(), q, name })
}

fn parse_link(link: &str) -> Result<J> {
    let scheme = link.split("://").next().unwrap_or("").to_ascii_lowercase();
    match scheme.as_str() {
        "vless" => vless(link),
        "hysteria2" | "hy2" => hysteria2(link),
        "trojan" => trojan(link),
        "ss" => shadowsocks(link),
        "vmess" => vmess(link),
        _ => bail!("неподдерживаемая схема {scheme}"),
    }
}

fn base(o: &mut Obj, kind: &str, l: &Link, default_port: u16) {
    let port = l.port.unwrap_or(default_port);
    let name = l.name.clone().unwrap_or_else(|| format!("{kind} {}:{port}", l.host));
    o.set("name", name.trim());
    o.set("type", kind);
    o.set("server", l.host.clone());
    o.set("port", port);
    o.set("udp", true);
}

/// Транспорт в терминах Clash: `type=ws` → `network: ws` + `ws-opts`,
/// `httpupgrade` → ws с `v2ray-http-upgrade`, `xhttp` → `xhttp-opts` и т.д.
fn transport(o: &mut Obj, q: &Q) {
    let net = q.get("type").map(String::as_str).unwrap_or("tcp");
    let path = q.get("path").cloned().unwrap_or_default();
    let host = q.get("host").cloned().filter(|h| !h.is_empty());
    match net {
        "ws" | "httpupgrade" => {
            o.set("network", "ws");
            let mut ws = json!({ "path": if path.is_empty() { "/".into() } else { path } });
            if let Some(h) = host {
                ws["headers"] = json!({ "Host": h });
            }
            if net == "httpupgrade" {
                ws["v2ray-http-upgrade"] = json!(true);
            }
            o.set("ws-opts", ws);
        }
        "grpc" => {
            o.set("network", "grpc");
            let svc = q.get("serviceName").cloned().unwrap_or(path);
            o.set("grpc-opts", json!({ "grpc-service-name": svc }));
        }
        "h2" | "http" => {
            o.set("network", "h2");
            let mut h2 = json!({ "path": if path.is_empty() { "/".into() } else { path } });
            if let Some(h) = host {
                h2["host"] = json!([h]);
            }
            o.set("h2-opts", h2);
        }
        "xhttp" | "splithttp" => {
            o.set("network", "xhttp");
            let mut x = json!({ "path": if path.is_empty() { "/".into() } else { path } });
            if let Some(h) = host {
                x["host"] = json!(h);
            }
            if let Some(mode) = q.get("mode").filter(|m| !m.is_empty()) {
                x["mode"] = json!(mode);
            }
            o.set("xhttp-opts", x);
        }
        _ => {
            if q.get("headerType").map(String::as_str) == Some("http") {
                o.set("network", "http");
                let mut http = json!({ "method": "GET", "path": [if path.is_empty() { "/".into() } else { path }] });
                if let Some(h) = host {
                    http["headers"] = json!({ "Host": [h] });
                }
                o.set("http-opts", http);
            } else {
                o.set("network", "tcp");
            }
        }
    }
}

fn tls(o: &mut Obj, q: &Q, sni_keys: &[&str]) {
    let sni = sni_keys.iter().find_map(|k| q.get(*k).filter(|v| !v.is_empty()));
    o.opt("servername", sni);
    o.opt("client-fingerprint", q.get("fp"));
    if let Some(alpn) = q.get("alpn").filter(|a| !a.is_empty()) {
        o.set("alpn", alpn.split(',').map(|s| J::String(s.trim().into())).collect::<Vec<_>>());
    }
    if is_true(q.get("allowInsecure")) || is_true(q.get("insecure")) {
        o.set("skip-cert-verify", true);
    }
}

fn vless(link: &str) -> Result<J> {
    let l = split_link(link)?;
    let mut o = Obj::new();
    base(&mut o, "vless", &l, 443);
    o.set("uuid", l.user.clone());
    o.opt("flow", l.q.get("flow"));
    if let Some(enc) = l.q.get("encryption").filter(|e| !e.is_empty() && *e != "none") {
        o.set("encryption", enc.clone());
    }
    let security = l.q.get("security").map(String::as_str).unwrap_or("none");
    if security == "tls" || security == "reality" {
        o.set("tls", true);
        tls(&mut o, &l.q, &["sni", "peer"]);
    }
    if security == "reality" {
        let pbk = l.q.get("pbk").cloned().unwrap_or_default();
        // Один сервер с битым ключом не даёт mihomo запуститься целиком — такой пропускаем.
        if URL_SAFE_NO_PAD.decode(pbk.trim_end_matches('=')).map_or(true, |k| k.len() != 32) {
            bail!("неверный ключ REALITY");
        }
        let mut r = json!({ "public-key": pbk });
        if let Some(sid) = l.q.get("sid").filter(|s| !s.is_empty()) {
            r["short-id"] = json!(sid);
        }
        o.set("reality-opts", r);
        if !o.0.contains_key("client-fingerprint") {
            o.set("client-fingerprint", "chrome");
        }
    }
    transport(&mut o, &l.q);
    Ok(o.done())
}

fn hysteria2(link: &str) -> Result<J> {
    let l = split_link(link)?;
    let mut o = Obj::new();
    base(&mut o, "hysteria2", &l, 443);
    o.set("password", l.user.clone());
    o.opt("sni", l.q.get("sni"));
    o.opt("obfs", l.q.get("obfs").filter(|v| *v != "none"));
    o.opt("obfs-password", l.q.get("obfs-password"));
    o.opt("ports", l.q.get("mport"));
    if let Some(alpn) = l.q.get("alpn").filter(|a| !a.is_empty()) {
        o.set("alpn", alpn.split(',').map(|s| J::String(s.trim().into())).collect::<Vec<_>>());
    }
    if is_true(l.q.get("insecure")) {
        o.set("skip-cert-verify", true);
    }
    Ok(o.done())
}

fn trojan(link: &str) -> Result<J> {
    let l = split_link(link)?;
    let mut o = Obj::new();
    base(&mut o, "trojan", &l, 443);
    o.set("password", l.user.clone());
    o.opt("sni", l.q.get("sni").or(l.q.get("peer")));
    o.opt("client-fingerprint", l.q.get("fp"));
    if let Some(alpn) = l.q.get("alpn").filter(|a| !a.is_empty()) {
        o.set("alpn", alpn.split(',').map(|s| J::String(s.trim().into())).collect::<Vec<_>>());
    }
    if is_true(l.q.get("allowInsecure")) {
        o.set("skip-cert-verify", true);
    }
    if l.q.get("security").map(String::as_str) == Some("reality") {
        let mut r = json!({ "public-key": l.q.get("pbk").cloned().unwrap_or_default() });
        if let Some(sid) = l.q.get("sid").filter(|s| !s.is_empty()) {
            r["short-id"] = json!(sid);
        }
        o.set("reality-opts", r);
    }
    transport(&mut o, &l.q);
    Ok(o.done())
}

fn shadowsocks(link: &str) -> Result<J> {
    let rest = &link[5..];
    let (rest, name) = match rest.split_once('#') {
        Some((r, n)) => (r, Some(pct(n))),
        None => (rest, None),
    };
    let rest = rest.split('?').next().unwrap_or(rest).trim_end_matches('/');
    let (userinfo, hostport) = match rest.rsplit_once('@') {
        Some((u, h)) => {
            let u = decode_b64(u).filter(|d| d.contains(':')).unwrap_or_else(|| pct(u));
            (u, h.to_string())
        }
        None => {
            let full = decode_b64(rest).ok_or_else(|| anyhow!("ss: не base64"))?;
            let (u, h) = full.rsplit_once('@').ok_or_else(|| anyhow!("ss: нет адреса"))?;
            (u.to_string(), h.to_string())
        }
    };
    let (method, password) = userinfo.split_once(':').ok_or_else(|| anyhow!("ss: нет пароля"))?;
    let (host, port) = hostport.rsplit_once(':').ok_or_else(|| anyhow!("ss: нет порта"))?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let port: u16 = port.parse()?;
    let name = name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| format!("ss {host}:{port}"));
    Ok(json!({
        "name": name.trim(), "type": "ss", "server": host, "port": port,
        "cipher": method, "password": password, "udp": true
    }))
}

fn vmess(link: &str) -> Result<J> {
    let raw = decode_b64(&link[8..]).ok_or_else(|| anyhow!("vmess: не base64"))?;
    let v: J = serde_json::from_str(&raw)?;
    let s = |k: &str| -> String {
        match &v[k] {
            J::String(s) => s.clone(),
            J::Number(n) => n.to_string(),
            _ => String::new(),
        }
    };
    let host = s("add");
    let port: u16 = s("port").parse()?;
    let mut o = Obj::new();
    let name = Some(s("ps")).filter(|n| !n.trim().is_empty()).unwrap_or_else(|| format!("vmess {host}:{port}"));
    o.set("name", name.trim());
    o.set("type", "vmess");
    o.set("server", host);
    o.set("port", port);
    o.set("udp", true);
    o.set("uuid", s("id"));
    o.set("alterId", s("aid").parse::<u32>().unwrap_or(0));
    o.set("cipher", Some(s("scy")).filter(|c| !c.is_empty()).unwrap_or_else(|| "auto".into()));
    let mut q = Q::new();
    q.insert("type".into(), s("net"));
    q.insert("path".into(), s("path"));
    q.insert("host".into(), s("host"));
    q.insert("headerType".into(), s("type"));
    q.insert("serviceName".into(), s("path"));
    q.insert("sni".into(), s("sni"));
    q.insert("fp".into(), s("fp"));
    q.insert("alpn".into(), s("alpn"));
    if s("tls") == "tls" {
        o.set("tls", true);
        tls(&mut o, &q, &["sni", "host"]);
    }
    transport(&mut o, &q);
    Ok(o.done())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vless_reality_xhttp() {
        let p = parse_link("vless://uuid-1@example.com:443?security=reality&pbk=SbVKOEMjK0sIlbwg4akyBg5mL5KZwwB-ed4eEE7YnRc&sid=ab&sni=my.site&fp=chrome&type=xhttp&path=%2Fx&mode=auto#%F0%9F%87%A9%F0%9F%87%AA%20DE").unwrap();
        assert_eq!(p["reality-opts"]["public-key"], "SbVKOEMjK0sIlbwg4akyBg5mL5KZwwB-ed4eEE7YnRc");
        assert!(parse_link("vless://u@h:1?security=reality&pbk=bad").is_err());
        assert_eq!(p["reality-opts"]["short-id"], "ab");
        assert_eq!(p["network"], "xhttp");
        assert_eq!(p["xhttp-opts"]["path"], "/x");
        assert_eq!(p["servername"], "my.site");
        assert_eq!(p["name"], "🇩🇪 DE");
    }

    #[test]
    fn vless_httpupgrade() {
        let p = parse_link("vless://u@h:80?type=httpupgrade&path=%2Fup&host=cdn.x").unwrap();
        assert_eq!(p["network"], "ws");
        assert_eq!(p["ws-opts"]["v2ray-http-upgrade"], true);
        assert_eq!(p["ws-opts"]["headers"]["Host"], "cdn.x");
    }

    #[test]
    fn hy2() {
        let p = parse_link("hysteria2://pa%40ss@1.2.3.4:8443?sni=a.b&obfs=salamander&obfs-password=zz&mport=20000-30000&insecure=1#hy").unwrap();
        assert_eq!(p["password"], "pa@ss");
        assert_eq!(p["ports"], "20000-30000");
        assert_eq!(p["obfs"], "salamander");
        assert_eq!(p["skip-cert-verify"], true);
    }

    #[test]
    fn ss_sip002() {
        let p = parse_link("ss://YWVzLTI1Ni1nY206cGFzcw@5.6.7.8:8388#my").unwrap();
        assert_eq!(p["cipher"], "aes-256-gcm");
        assert_eq!(p["password"], "pass");
    }

    #[test]
    fn base64_list() {
        let list = STANDARD.encode("vless://a@h:1?type=ws#one\nvless://a@h:1?type=ws#one\n");
        let parsed = parse(&list).unwrap();
        assert_eq!(parsed.proxy_count, 2);
        assert!(parsed.yaml.contains("one 2"));
    }
}
