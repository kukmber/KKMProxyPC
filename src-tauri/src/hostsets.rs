//! Готовые наборы доменов для обхода блокировок: отмечаешь галочкой нужные
//! сервисы вместо того, чтобы вспоминать и вписывать адреса руками.

use serde::Serialize;

pub struct HostSet {
    pub id: &'static str,
    pub title: &'static str,
    pub about: &'static str,
    pub hosts: &'static [&'static str],
}

pub const HOST_SETS: &[HostSet] = &[
    HostSet {
        id: "discord",
        title: "Discord",
        about: "Сам Discord, его CDN и голосовые серверы",
        hosts: &[
            "discord.com",
            "discordapp.com",
            "discordapp.net",
            "discord.gg",
            "discord.media",
            "discordcdn.com",
            "discord-attachments-uploads-prd.storage.googleapis.com",
            "cloudflare.steamstatic.com",
        ],
    },
    HostSet {
        id: "youtube",
        title: "YouTube и Google",
        about: "Видео, картинки и сервисы Google",
        hosts: &[
            "youtube.com",
            "youtu.be",
            "ytimg.com",
            "ggpht.com",
            "googlevideo.com",
            "youtubei.googleapis.com",
            "yt3.ggpht.com",
            "jnn-pa.googleapis.com",
            "googleusercontent.com",
        ],
    },
    HostSet {
        id: "telegram",
        title: "Telegram",
        about: "Сайт, веб-версия и серверы загрузки",
        hosts: &["telegram.org", "t.me", "telegram.me", "telesco.pe", "tdesktop.com", "telegra.ph"],
    },
    HostSet {
        id: "cloudflare",
        title: "Cloudflare",
        about: "DNS, Workers и хранилище — на них держится много сайтов",
        hosts: &["cloudflare.com", "cloudflare-dns.com", "workers.dev", "r2.dev", "cdnjs.cloudflare.com", "one.one.one.one"],
    },
    HostSet {
        id: "twitch",
        title: "Twitch",
        about: "Трансляции и их CDN",
        hosts: &["twitch.tv", "ttvnw.net", "jtvnw.net", "twitchcdn.net", "twitchsvc.net"],
    },
    HostSet {
        id: "x",
        title: "X (Twitter)",
        about: "Сайт и картинки",
        hosts: &["x.com", "twitter.com", "twimg.com", "t.co"],
    },
    HostSet {
        id: "meta",
        title: "Instagram и Facebook",
        about: "Сайты Meta и их CDN",
        hosts: &["instagram.com", "cdninstagram.com", "facebook.com", "fbcdn.net", "whatsapp.com", "whatsapp.net"],
    },
    HostSet {
        id: "spotify",
        title: "Spotify",
        about: "Музыка и обложки",
        hosts: &["spotify.com", "scdn.co", "spotifycdn.com", "audio-ak-spotify-com.akamaized.net"],
    },
    HostSet {
        id: "ai",
        title: "ChatGPT и Claude",
        about: "Сервисы, закрытые для России",
        hosts: &["openai.com", "chatgpt.com", "oaistatic.com", "oaiusercontent.com", "anthropic.com", "claude.ai"],
    },
];

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HostSetInfo {
    pub id: String,
    pub title: String,
    pub about: String,
    pub count: usize,
}

pub fn list() -> Vec<HostSetInfo> {
    HOST_SETS
        .iter()
        .map(|s| HostSetInfo {
            id: s.id.into(),
            title: s.title.into(),
            about: s.about.into(),
            count: s.hosts.len(),
        })
        .collect()
}

/// Собирает итоговый список: выбранные наборы плюс то, что вписал пользователь.
/// Повторы убираются, порядок сохраняется.
pub fn collect(selected: &[String], custom: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    let mut push = |host: &str| {
        let h = host.trim().trim_start_matches("*.").to_ascii_lowercase();
        if !h.is_empty() && !h.starts_with('#') && seen.insert(h.clone()) {
            out.push(h);
        }
    };
    for id in selected {
        if let Some(set) = HOST_SETS.iter().find(|s| s.id == *id) {
            for h in set.hosts {
                push(h);
            }
        }
    }
    for line in custom.lines() {
        push(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_and_dedupes() {
        let hosts = collect(&["discord".into(), "нет такого".into()], "discord.com\n*.example.org\n\n# коммент\n");
        assert!(hosts.contains(&"discord.gg".to_string()));
        assert!(hosts.contains(&"example.org".to_string()));
        assert_eq!(hosts.iter().filter(|h| *h == "discord.com").count(), 1);
        assert!(!hosts.iter().any(|h| h.starts_with('#')));
    }

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<&str> = HOST_SETS.iter().map(|s| s.id).collect();
        ids.sort_unstable();
        let n = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }
}
