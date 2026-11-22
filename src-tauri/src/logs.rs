use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter};

const MAX_LINES: usize = 3000;

/// Сколько мегабайт журнала держать в памяти. Меняется в настройках.
static MAX_MB: AtomicUsize = AtomicUsize::new(1);

/// Ограничение по размеру: строки в журнале короткие, поэтому считаем
/// «в среднем 160 байт на строку» — точность здесь не нужна.
fn max_lines() -> usize {
    let mb = MAX_MB.load(Ordering::Relaxed).max(1);
    (mb * 1024 * 1024 / 160).clamp(MAX_LINES / 2, 200_000)
}

/// Меняет предел журнала. Значение приходит из настроек Telegram, потому что
/// там же живёт переключатель подробного журнала ядра.
pub fn set_max_mb(mb: u32) {
    MAX_MB.store(mb.clamp(1, 100) as usize, Ordering::Relaxed);
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub id: u64,
    pub ts: u64,
    /// vpn | app (позже: tg, dpi)
    pub source: String,
    /// debug | info | warning | error
    pub level: String,
    pub msg: String,
}

#[derive(Default)]
pub struct Logs {
    inner: Mutex<(u64, VecDeque<LogLine>)>,
}

impl Logs {
    pub fn push(&self, app: &AppHandle, source: &str, level: &str, msg: impl Into<String>) {
        let line = {
            let mut g = self.inner.lock().unwrap();
            g.0 += 1;
            let line = LogLine {
                id: g.0,
                ts: crate::util::now_ms(),
                source: source.into(),
                level: level.into(),
                msg: msg.into(),
            };
            if g.1.len() >= max_lines() {
                g.1.pop_front();
            }
            g.1.push_back(line.clone());
            line
        };
        let _ = app.emit("log", &line);
    }

    pub fn get(&self, source: Option<&str>) -> Vec<LogLine> {
        let g = self.inner.lock().unwrap();
        g.1.iter()
            .filter(|l| source.map_or(true, |s| l.source == s))
            .cloned()
            .collect()
    }

    pub fn clear(&self, source: Option<&str>) {
        let mut g = self.inner.lock().unwrap();
        match source {
            Some(s) => g.1.retain(|l| l.source != s),
            None => g.1.clear(),
        }
    }
}

/// Разбирает строку журнала mihomo вида
/// `time="..." level=info msg="Start initial configuration in progress"`.
pub fn parse_mihomo_line(line: &str) -> (String, String) {
    let level = line
        .split_once("level=")
        .and_then(|(_, rest)| rest.split_whitespace().next())
        .unwrap_or("info")
        .to_string();
    let msg = match line.split_once("msg=") {
        Some((_, rest)) => unquote(rest.trim()),
        None => line.trim().to_string(),
    };
    let level = match level.as_str() {
        "warn" => "warning".to_string(),
        "fatal" | "panic" => "error".to_string(),
        _ => level,
    };
    (level, msg)
}

fn unquote(s: &str) -> String {
    let Some(inner) = s.strip_prefix('"') else {
        return s.to_string();
    };
    let inner = inner.strip_suffix('"').unwrap_or(inner);
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}
