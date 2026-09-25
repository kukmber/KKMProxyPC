//! Итоговый конфиг mihomo: берём конфиг профиля целиком и поверх
//! накладываем то, чем управляет приложение (порты, контроллер, DNS, TUN).

use serde_yaml::{Mapping, Value};
use std::path::{Path, PathBuf};

pub struct RuntimeOpts<'a> {
    pub mixed_port: u16,
    pub controller_port: u16,
    pub secret: &'a str,
    pub mode: &'a str,
    pub tun: bool,
}

/// Правило, по которому проверяется готовность ядра: запрос к собственному
/// контроллеру через SOCKS-порт ядра уходит напрямую и не зависит от VPN-сервера.
pub const LOOPBACK_RULE: &str = "IP-CIDR,127.0.0.1/32,DIRECT,no-resolve";

const RU_DNS: &str = "77.88.8.8";

fn k(s: &str) -> Value {
    Value::String(s.to_string())
}

fn strs(items: &[&str]) -> Value {
    Value::Sequence(items.iter().map(|s| k(s)).collect())
}

fn map_mut<'a>(m: &'a mut Mapping, key: &str) -> &'a mut Mapping {
    if !m.get(key).is_some_and(Value::is_mapping) {
        m.insert(k(key), Value::Mapping(Mapping::new()));
    }
    m.get_mut(key).and_then(Value::as_mapping_mut).unwrap()
}

pub fn apply_overrides(cfg: &mut Mapping, o: &RuntimeOpts) {
    for key in [
        "port", "socks-port", "redir-port", "tproxy-port", "mixed-port", "external-controller",
        "external-controller-tls", "external-controller-unix", "external-controller-pipe",
        "external-ui", "external-ui-url", "secret", "bind-address", "interface-name",
    ] {
        cfg.remove(key);
    }
    cfg.insert(k("mixed-port"), Value::from(o.mixed_port));
    cfg.insert(k("allow-lan"), Value::Bool(false));
    cfg.insert(k("external-controller"), k(&format!("127.0.0.1:{}", o.controller_port)));
    cfg.insert(k("secret"), k(o.secret));
    let mut cors = Mapping::new();
    cors.insert(k("allow-origins"), strs(&["*"]));
    cors.insert(k("allow-private-network"), Value::Bool(true));
    cfg.insert(k("external-controller-cors"), Value::Mapping(cors));
    cfg.insert(k("mode"), k(o.mode));
    cfg.insert(k("log-level"), k("info"));
    // Нужен для правил PROCESS-NAME и для имени программы в списке соединений.
    cfg.insert(k("find-process-mode"), k("always"));
    if !cfg.contains_key("unified-delay") {
        cfg.insert(k("unified-delay"), Value::Bool(true));
    }
    if !cfg.contains_key("tcp-concurrent") {
        cfg.insert(k("tcp-concurrent"), Value::Bool(true));
    }
    let profile = map_mut(cfg, "profile");
    profile.insert(k("store-selected"), Value::Bool(true));

    apply_dns(cfg, o.tun);
    apply_tun(cfg, o.tun);

    let rules = cfg
        .entry(k("rules"))
        .or_insert_with(|| Value::Sequence(vec![]));
    if let Value::Sequence(seq) = rules {
        seq.retain(|r| r.as_str() != Some(LOOPBACK_RULE));
        seq.insert(0, k(LOOPBACK_RULE));
    }
}

fn apply_dns(cfg: &mut Mapping, tun: bool) {
    let dns = map_mut(cfg, "dns");
    let fresh = dns.is_empty();
    dns.insert(k("enable"), Value::Bool(true));
    if fresh {
        dns.insert(k("ipv6"), Value::Bool(false));
        dns.insert(k("enhanced-mode"), k("fake-ip"));
        dns.insert(k("fake-ip-range"), k("198.18.0.1/16"));
        dns.insert(
            k("fake-ip-filter"),
            strs(&["*.lan", "+.local", "+.msftconnecttest.com", "+.msftncsi.com", "localhost.ptlink.com"]),
        );
    }
    if !dns.get("default-nameserver").is_some_and(Value::is_sequence) {
        dns.insert(k("default-nameserver"), strs(&[RU_DNS, "1.1.1.1"]));
    }
    if !dns.get("nameserver").is_some_and(Value::is_sequence) {
        dns.insert(k("nameserver"), strs(&["https://1.1.1.1/dns-query", "https://dns.google/dns-query"]));
    }
    // Без этого запросы DNS ядра идут напрямую, а зарубежный DoH из России часто недоступен.
    dns.insert(k("respect-rules"), Value::Bool(true));
    // ...но адрес самого VPN-сервера должен разрешаться напрямую, российским резолвером.
    let psn = dns
        .entry(k("proxy-server-nameserver"))
        .or_insert_with(|| Value::Sequence(vec![]));
    if let Value::Sequence(seq) = psn {
        if !seq.iter().any(|v| v.as_str() == Some(RU_DNS)) {
            seq.insert(0, k(RU_DNS));
        }
    }
    if tun {
        dns.remove("listen");
    }
}

fn apply_tun(cfg: &mut Mapping, tun: bool) {
    let mut t = Mapping::new();
    t.insert(k("enable"), Value::Bool(tun));
    if tun {
        t.insert(k("stack"), k("mixed"));
        t.insert(k("auto-route"), Value::Bool(true));
        t.insert(k("auto-detect-interface"), Value::Bool(true));
        t.insert(k("strict-route"), Value::Bool(true));
        t.insert(k("dns-hijack"), strs(&["any:53", "tcp://any:53"]));
    }
    cfg.insert(k("tun"), Value::Mapping(t));
}

#[derive(Clone, Debug)]
pub struct Download {
    pub name: String,
    pub url: String,
    pub path: PathBuf,
    /// Файл правил (его можно временно исключить) или список серверов.
    pub rule: bool,
}

fn sanitize(name: &str) -> String {
    name.chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' }).collect()
}

/// Проставляет явные пути HTTP-провайдерам и возвращает те, чьих файлов ещё нет.
pub fn missing_providers(cfg: &mut Mapping, home: &Path) -> Vec<Download> {
    let mut out = Vec::new();
    for (section, dir, rule) in [("rule-providers", "rules", true), ("proxy-providers", "proxies", false)] {
        let Some(Value::Mapping(providers)) = cfg.get_mut(section) else { continue };
        for (name, p) in providers.iter_mut() {
            let (Some(name), Some(p)) = (name.as_str(), p.as_mapping_mut()) else { continue };
            if p.get("type").and_then(Value::as_str) != Some("http") {
                continue;
            }
            let Some(url) = p.get("url").and_then(Value::as_str).map(str::to_string) else { continue };
            let rel = match p.get("path").and_then(Value::as_str) {
                Some(path) => path.to_string(),
                None => {
                    let ext = match p.get("format").and_then(Value::as_str) {
                        Some("mrs") => "mrs",
                        Some("text") => "txt",
                        _ => "yaml",
                    };
                    let rel = format!("./{dir}/{}.{ext}", sanitize(name));
                    p.insert(k("path"), k(&rel));
                    rel
                }
            };
            let path = home.join(rel.trim_start_matches("./"));
            if std::fs::metadata(&path).map_or(true, |m| m.len() == 0) {
                out.push(Download { name: name.to_string(), url, path, rule });
            }
        }
    }
    out
}

/// Убирает rule-provider'ы вместе со всеми правилами, которые на них ссылаются.
pub fn exclude_rule_providers(cfg: &mut Mapping, names: &[String]) {
    if names.is_empty() {
        return;
    }
    if let Some(Value::Mapping(p)) = cfg.get_mut("rule-providers") {
        for n in names {
            p.remove(n.as_str());
        }
    }
    let refers = |rule: &str| {
        names.iter().any(|n| {
            let tag = format!("RULE-SET,{n}");
            rule.match_indices(&tag).any(|(i, _)| {
                matches!(rule[i + tag.len()..].chars().next(), None | Some(',') | Some(')'))
            })
        })
    };
    let filter = |seq: &mut Vec<Value>| seq.retain(|r| !r.as_str().is_some_and(|s| refers(s)));
    if let Some(Value::Sequence(rules)) = cfg.get_mut("rules") {
        filter(rules);
    }
    if let Some(Value::Mapping(sub)) = cfg.get_mut("sub-rules") {
        for (_, v) in sub.iter_mut() {
            if let Value::Sequence(rules) = v {
                filter(rules);
            }
        }
    }
    if let Some(Value::Mapping(dns)) = cfg.get_mut("dns") {
        if let Some(Value::Mapping(policy)) = dns.get_mut("nameserver-policy") {
            policy.retain(|key, _| {
                let key = key.as_str().unwrap_or("");
                !names.iter().any(|n| key.split(',').any(|part| part.trim() == format!("rule-set:{n}")))
            });
        }
    }
}

/// Geo-базы, которые ядро иначе стало бы скачивать само при старте.
pub fn missing_geo(cfg: &Mapping, home: &Path) -> Vec<Download> {
    let text = serde_yaml::to_string(cfg).unwrap_or_default().to_lowercase();
    let geox = cfg.get("geox-url").and_then(Value::as_mapping);
    let url = |key: &str, default: &str| {
        geox.and_then(|g| g.get(key)).and_then(Value::as_str).unwrap_or(default).to_string()
    };
    const BASE: &str = "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/";
    let geodata_mode = cfg.get("geodata-mode").and_then(Value::as_bool).unwrap_or(false);
    let mut want = Vec::new();
    if text.contains("geosite") {
        want.push(("GeoSite.dat", url("geosite", &format!("{BASE}geosite.dat"))));
    }
    if text.contains("geoip") {
        if geodata_mode {
            want.push(("GeoIP.dat", url("geoip", &format!("{BASE}geoip.dat"))));
        } else {
            want.push(("Country.mmdb", url("mmdb", &format!("{BASE}country.mmdb"))));
        }
    }
    let existing: Vec<String> = std::fs::read_dir(home)
        .map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().to_lowercase()).collect())
        .unwrap_or_default();
    want.into_iter()
        .filter(|(file, _)| !existing.contains(&file.to_lowercase()))
        .map(|(file, url)| Download { name: file.into(), url, path: home.join(file), rule: false })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclude_removes_rules_and_provider() {
        let mut cfg: Mapping = serde_yaml::from_str(
            "rule-providers:\n  ads: {type: http, url: 'http://x'}\n  adsx: {type: http, url: 'http://y'}\n\
             rules:\n  - RULE-SET,ads,REJECT\n  - RULE-SET,adsx,REJECT\n  - AND,((RULE-SET,ads),(NETWORK,UDP)),REJECT\n  - MATCH,DIRECT\n",
        )
        .unwrap();
        exclude_rule_providers(&mut cfg, &["ads".into()]);
        let rules: Vec<&str> = cfg["rules"].as_sequence().unwrap().iter().filter_map(Value::as_str).collect();
        assert_eq!(rules, vec!["RULE-SET,adsx,REJECT", "MATCH,DIRECT"]);
        assert!(cfg["rule-providers"].as_mapping().unwrap().get("ads").is_none());
    }

    /// Собирает итоговый конфиг из ссылки для ручной проверки живым mihomo:
    /// `KKM_DUMP=путь cargo test dump_runtime_config -- --ignored`
    #[test]
    #[ignore]
    fn dump_runtime_config() {
        let out = std::env::var("KKM_DUMP").expect("KKM_DUMP");
        let parsed = crate::subscription::parse(
            "vless://00000000-0000-0000-0000-000000000000@203.0.113.1:443?security=reality&pbk=SbVKOEMjK0sIlbwg4akyBg5mL5KZwwB-ed4eEE7YnRc&sid=01&sni=example.com&type=xhttp&path=%2Fx#🇩🇪 Test DE",
        )
        .unwrap();
        let mut cfg: Mapping = serde_yaml::from_str(&parsed.yaml).unwrap();
        apply_overrides(&mut cfg, &RuntimeOpts { mixed_port: 17890, controller_port: 19090, secret: "test", mode: "rule", tun: false });
        std::fs::write(out, serde_yaml::to_string(&cfg).unwrap()).unwrap();
    }

    #[test]
    fn overrides_put_loopback_first() {
        let mut cfg: Mapping = serde_yaml::from_str("port: 1\nrules:\n  - MATCH,PROXY\n").unwrap();
        apply_overrides(&mut cfg, &RuntimeOpts { mixed_port: 7890, controller_port: 9, secret: "s", mode: "rule", tun: false });
        assert!(cfg.get("port").is_none());
        assert_eq!(cfg["rules"][0].as_str(), Some(LOOPBACK_RULE));
        assert_eq!(cfg["dns"]["respect-rules"], Value::Bool(true));
        assert_eq!(cfg["dns"]["proxy-server-nameserver"][0].as_str(), Some(RU_DNS));
    }
}
