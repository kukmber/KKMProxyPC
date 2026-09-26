//! Обход DPI без VPN: zapret (winws.exe + WinDivert).
//!
//! winws перехватывает исходящие пакеты драйвером WinDivert и «портит» их так,
//! что DPI провайдера не может разобрать запрос, а сервер — понимает. Поэтому
//! нужны права администратора: драйвер ставится в систему.

use crate::child::{self, Running, Spawn};
use crate::state::AppState;
use crate::util::now_ms;
use crate::{cores, winsys};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::ffi::OsString;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ZapretStatus {
    /// stopped | starting | running | stopping
    pub state: String,
    pub started_at: Option<u64>,
    pub error: Option<String>,
    /// Название применённой стратегии.
    pub strategy: Option<String>,
}

pub struct ZapretManager {
    status: Mutex<ZapretStatus>,
    running: tokio::sync::Mutex<Option<Running>>,
    op: tokio::sync::Mutex<()>,
}

impl ZapretManager {
    pub fn new() -> Self {
        Self {
            status: Mutex::new(ZapretStatus { state: "stopped".into(), ..Default::default() }),
            running: tokio::sync::Mutex::new(None),
            op: tokio::sync::Mutex::new(()),
        }
    }

    pub fn status(&self) -> ZapretStatus {
        self.status.lock().unwrap().clone()
    }
}

fn set_status(app: &AppHandle, f: impl FnOnce(&mut ZapretStatus)) {
    let st = app.state::<AppState>();
    let snapshot = {
        let mut s = st.zapret.status.lock().unwrap();
        f(&mut s);
        s.clone()
    };
    let _ = app.emit("zapret-status", &snapshot);
}

fn log(app: &AppHandle, level: &str, msg: impl Into<String>) {
    app.state::<AppState>().logs.push(app, "dpi", level, msg);
}

/// Стратегия — один .bat-файл из набора Flowseal: имя файла и строка запуска
/// winws внутри него. Свои стратегии не выдумываем: перебором проверяется то,
/// что уже собрано и обкатано сообществом.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StrategyInfo {
    pub id: String,
    pub title: String,
    pub about: String,
}

/// Идентификатор своей стратегии.
pub const CUSTOM: &str = "custom";

/// Папка ядра: внутри bin\winws.exe, lists\ и .bat-файлы стратегий.
fn core_dir(app: &AppHandle) -> Option<std::path::PathBuf> {
    cores::installed_dir(app, "zapret")
}

/// Разбирает .bat: склеивает строку запуска winws (перенос — символ `^`)
/// и подставляет переменные, которые задаёт сам набор.
fn parse_bat(text: &str, dir: &Path) -> Option<Vec<String>> {
    let lines: Vec<&str> = text.lines().collect();
    let idx = lines.iter().position(|l| l.contains("winws.exe"))?;
    let mut joined = String::new();
    for line in &lines[idx..] {
        let trimmed = line.trim_end();
        let more = trimmed.ends_with('^');
        joined.push(' ');
        joined.push_str(trimmed.trim_end_matches('^').trim());
        if !more {
            break;
        }
    }
    // Всё, что идёт после пути к winws.exe, — это его аргументы.
    let args_part = joined.split("winws.exe\"").nth(1)?;
    let bin = dir.join("bin").to_string_lossy().into_owned() + "\\";
    let lists = dir.join("lists").to_string_lossy().into_owned() + "\\";
    let replaced = args_part
        .replace("%BIN%", &bin)
        .replace("%LISTS%", &lists)
        // Фильтр игр в наборе по умолчанию выключен и подменяется портом-заглушкой.
        .replace("%GameFilterTCP%", "12")
        .replace("%GameFilterUDP%", "12");
    let args = split_args(&replaced);
    (!args.is_empty()).then_some(args)
}

/// Короткое описание стратегии: чем именно она «портит» пакеты.
fn describe(text: &str) -> String {
    if text.to_uppercase().contains("NOT RECOMMENDED") {
        return "Не рекомендуется — только если ничего другое не помогло".into();
    }
    let mut methods: Vec<String> = Vec::new();
    for part in text.split("--dpi-desync=").skip(1) {
        let m = part.split_whitespace().next().unwrap_or("");
        if !m.is_empty() && !methods.iter().any(|x| x == m) {
            methods.push(m.to_string());
        }
    }
    if methods.is_empty() {
        "Набор Flowseal".into()
    } else {
        format!("Приёмы: {}", methods.join(", "))
    }
}

/// Список стратегий — все .bat-файлы из папки ядра.
pub fn list_strategies(app: &AppHandle) -> Vec<StrategyInfo> {
    let Some(dir) = core_dir(app) else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut out: Vec<StrategyInfo> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str())? != "bat" {
                return None;
            }
            let id = path.file_stem()?.to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&path).ok()?;
            parse_bat(&text, &dir)?;
            Some(StrategyInfo { title: id.clone(), about: describe(&text), id })
        })
        .collect();
    // «general» первой, дальше по алфавиту с учётом чисел: ALT2 раньше ALT10.
    out.sort_by_key(|s| (s.id != "general", natural_key(&s.id)));
    out
}

/// Ключ сортировки, в котором числа сравниваются как числа.
fn natural_key(name: &str) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    let mut chars = name.chars().peekable();
    while chars.peek().is_some() {
        let text: String = std::iter::from_fn(|| chars.next_if(|c| !c.is_ascii_digit())).collect();
        let digits: String = std::iter::from_fn(|| chars.next_if(|c| c.is_ascii_digit())).collect();
        out.push((text.to_lowercase(), digits.parse().unwrap_or(0)));
    }
    out
}

/// Аргументы стратегии по её идентификатору.
pub fn args_of(app: &AppHandle, id: &str) -> Result<Vec<String>> {
    if id == CUSTOM {
        let raw = app.state::<AppState>().settings.lock().unwrap().zapret_custom.trim().to_string();
        if raw.is_empty() {
            bail!("Своя стратегия пустая — впишите аргументы winws или выберите готовую");
        }
        return Ok(split_args(&raw));
    }
    let dir = core_dir(app).context("ядро zapret не установлено")?;
    let path = dir.join(format!("{id}.bat"));
    let text = std::fs::read_to_string(&path).with_context(|| format!("нет файла стратегии {id}.bat"))?;
    parse_bat(&text, &dir).with_context(|| format!("не удалось разобрать {id}.bat"))
}

/// Название выбранной стратегии для подписи в интерфейсе.
fn strategy_title(id: &str) -> String {
    if id == CUSTOM {
        "Своя".into()
    } else {
        id.to_string()
    }
}

/// Превращает строки аргументов в то, что примет процесс.
/// `@fake/<файл>` в своей стратегии заменяется путём к заготовке пакета.
fn build_args(args: &[String], dir: &Path) -> Vec<OsString> {
    args.iter()
        .map(|a| match a.split_once("=@") {
            Some((key, rel)) => {
                let mut v = OsString::from(key);
                v.push("=");
                v.push(dir.join("bin").join(rel.trim_start_matches("fake/")));
                v
            }
            None => OsString::from(a.as_str()),
        })
        .collect()
}

/// Разбивает строку аргументов по пробелам и переводам строк, уважая кавычки.
pub fn split_args(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut has_token = false;
    for c in raw.chars() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => cur.push(c),
            (None, '"') | (None, '\'') => {
                quote = Some(c);
                has_token = true;
            }
            (None, _) if c.is_whitespace() => {
                if has_token {
                    out.push(std::mem::take(&mut cur));
                    has_token = false;
                }
            }
            (None, _) => {
                cur.push(c);
                has_token = true;
            }
        }
    }
    if has_token {
        out.push(cur);
    }
    out
}

/// Набор сам создаёт пользовательские списки — но делает это в service.bat,
/// который мы не запускаем. Поэтому создаём те же файлы сами: без них winws
/// не стартует вовсе («cannot access ipset file»), и не работает ни одна
/// стратегия.
pub fn ensure_lists(app: &AppHandle) -> Result<()> {
    let Some(dir) = core_dir(app) else { return Ok(()) };
    let lists = dir.join("lists");
    std::fs::create_dir_all(&lists)?;
    // Значения те же, что подставляет сам набор.
    for (name, body) in [
        ("ipset-exclude-user.txt", "203.0.113.113/32\r\n"),
        ("list-exclude-user.txt", "domain.example.abc\r\n"),
    ] {
        let path = lists.join(name);
        if std::fs::metadata(&path).map_or(true, |m| m.len() == 0) {
            crate::util::write_atomic(&path, body)?;
        }
    }
    rebuild_hostlist(app)?;
    Ok(())
}

/// Свой список сайтов набор Flowseal читает из lists\list-general-user.txt —
/// туда и пишем отмеченные наборы вместе со своими адресами.
fn user_list_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    Some(core_dir(app)?.join("lists").join("list-general-user.txt"))
}

/// Пересобирает свой список из отмеченных наборов и вписанных адресов.
/// Возвращает число записей.
pub fn rebuild_hostlist(app: &AppHandle) -> Result<usize> {
    let st = app.state::<AppState>();
    let (sets, custom) = {
        let s = st.settings.lock().unwrap();
        (s.zapret_sets.clone(), s.zapret_custom_hosts.clone())
    };
    let hosts = crate::hostsets::collect(&sets, &custom);
    let Some(path) = user_list_path(app) else { return Ok(hosts.len()) };
    // Пустой файл набор считает ошибкой, поэтому строка-пояснение остаётся всегда.
    let mut text = String::from("# Список KKMProxy: наборы и свои адреса\r\n");
    if hosts.is_empty() {
        text.push_str("domain.example.abc\r\n");
    } else {
        text.push_str(&hosts.join("\r\n"));
    }
    crate::util::write_atomic(&path, text)?;
    Ok(hosts.len())
}

pub async fn start(app: &AppHandle) -> Result<()> {
    let st = app.state::<AppState>();
    let _op = st.zapret.op.lock().await;
    if st.zapret.status().state == "running" {
        return Ok(());
    }
    set_status(app, |s| *s = ZapretStatus { state: "starting".into(), ..Default::default() });
    match start_inner(app).await {
        Ok(()) => Ok(()),
        Err(e) => {
            let msg = format!("{e:#}");
            log(app, "error", format!("Не удалось включить обход: {msg}"));
            if let Some(r) = st.zapret.running.lock().await.take() {
                r.stop().await;
            }
            set_status(app, |s| *s = ZapretStatus { state: "stopped".into(), error: Some(msg), ..Default::default() });
            Err(e)
        }
    }
}

async fn start_inner(app: &AppHandle) -> Result<()> {
    // WinDivert ставит драйвер в систему — без прав администратора никак.
    if !winsys::is_elevated() {
        bail!("Нужны права администратора: обход работает через драйвер WinDivert");
    }
    // Два обхода DPI одновременно перехватывают одни и те же пакеты через
    // WinDivert и рвут соединения — проверено: интернет пропадает целиком.
    if let Some((pid, owner)) = winsys::find_process("winws.exe") {
        bail!("Обход уже запущен другой программой:
{owner}

Два обхода блокировок одновременно рвут сеть. Выключите обход в той программе и попробуйте снова (PID {pid}).");
    }
    let st = app.state::<AppState>();
    let exe = cores::ensure(app, "zapret").await?;
    let dir = core_dir(app).context("нет папки ядра")?;
    let id = st.settings.lock().unwrap().zapret_strategy.clone();
    let title = strategy_title(&id);
    // Списки должны существовать до запуска — иначе ядро откажется стартовать.
    ensure_lists(app)?;
    let args = build_args(&args_of(app, &id)?, &dir);

    let exited = Arc::new(AtomicBool::new(false));
    let running = child::spawn(
        app,
        Spawn {
            exe: &exe,
            args: args.iter().map(|a| a.as_os_str()).collect(),
            cwd: Some(&dir),
            source: "dpi",
            parse: parse_line,
            exited: exited.clone(),
        },
        |app, code| async move { on_crash(&app, code).await },
    )
    .await?;
    *st.zapret.running.lock().await = Some(running);

    // Своего сигнала готовности у winws нет: если параметры или драйвер
    // не подошли, он выходит в первые же мгновения. Даём ему это время.
    tokio::time::sleep(Duration::from_millis(1200)).await;
    if exited.load(Ordering::SeqCst) {
        bail!("ядро завершилось сразу после запуска — подробности в журнале");
    }
    set_status(app, |st| {
        *st = ZapretStatus {
            state: "running".into(),
            started_at: Some(now_ms()),
            error: None,
            strategy: Some(title.clone()),
        }
    });
    log(app, "info", format!("Обход блокировок включён, стратегия «{title}»"));
    Ok(())
}

pub async fn stop(app: &AppHandle) -> Result<()> {
    let st = app.state::<AppState>();
    let _op = st.zapret.op.lock().await;
    if st.zapret.status().state == "stopped" {
        return Ok(());
    }
    set_status(app, |s| s.state = "stopping".into());
    if let Some(r) = st.zapret.running.lock().await.take() {
        r.stop().await;
    }
    set_status(app, |s| *s = ZapretStatus { state: "stopped".into(), ..Default::default() });
    log(app, "info", "Обход блокировок выключен");
    Ok(())
}

async fn on_crash(app: &AppHandle, code: Option<i32>) {
    let st = app.state::<AppState>();
    if st.zapret.status().state != "running" {
        return;
    }
    st.zapret.running.lock().await.take();
    let msg = format!("Обход неожиданно завершился (код {})", code.map_or("?".into(), |c| c.to_string()));
    log(app, "error", &msg);
    set_status(app, |s| *s = ZapretStatus { state: "stopped".into(), error: Some(msg), ..Default::default() });
}

/// Проверяет набор аргументов без запуска: winws разбирает их и выходит.
pub async fn check(app: &AppHandle, raw_args: &[String]) -> Result<()> {
    let exe = cores::ensure(app, "zapret").await?;
    let dir = core_dir(app).context("нет папки ядра")?;
    ensure_lists(app)?;
    let mut args = build_args(raw_args, &dir);
    args.insert(0, OsString::from("--dry-run"));
    let out = tokio::process::Command::new(&exe)
        .args(&args)
        .current_dir(&dir)
        .creation_flags(winsys::CREATE_NO_WINDOW)
        .output()
        .await?;
    if !out.status.success() {
        let text = String::from_utf8_lossy(&out.stderr);
        let line = text
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("ошибка в параметрах стратегии");
        bail!("{}", line.trim());
    }
    Ok(())
}

/// winws пишет простым текстом, уровень отмечает словами в начале строки.
fn parse_line(line: &str) -> (String, String) {
    let l = line.trim();
    let low = l.to_ascii_lowercase();
    let level = if low.starts_with("error") || low.contains("failed") || low.contains("cannot") {
        "error"
    } else if low.starts_with("warning") || low.starts_with("warn") {
        "warning"
    } else {
        "info"
    };
    (level.into(), l.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Строка запуска из настоящего .bat набора Flowseal (сокращённая).
    const BAT: &str = concat!(
        "@echo off\r\n",
        "chcp 65001 > nul\r\n",
        ":: 65001 - UTF-8\r\n",
        "set \"BIN=%~dp0bin\\\"\r\n",
        "start \"zapret: %~n0\" /min \"%BIN%winws.exe\" --wf-tcp=80,443,%GameFilterTCP% --wf-udp=443 ^\r\n",
        "--filter-udp=443 --hostlist=\"%LISTS%list-general.txt\" --dpi-desync=fake --new ^\r\n",
        "--filter-tcp=443 --dpi-desync=multisplit --dpi-desync-split-seqovl-pattern=\"%BIN%tls_clienthello_www_google_com.bin\"\r\n"
    );

    #[test]
    fn parses_flowseal_bat() {
        let args = parse_bat(BAT, Path::new(r"C:\cores\zapret")).unwrap();
        assert_eq!(args[0], "--wf-tcp=80,443,12", "фильтр игр заменяется заглушкой");
        assert!(args.contains(&r"--hostlist=C:\cores\zapret\lists\list-general.txt".to_string()));
        assert!(args.contains(
            &r"--dpi-desync-split-seqovl-pattern=C:\cores\zapret\bin\tls_clienthello_www_google_com.bin".to_string()
        ));
        // Переносы строк склеены: профили идут одной командой.
        assert_eq!(args.iter().filter(|a| *a == "--new").count(), 1);
    }

    #[test]
    fn describes_by_methods() {
        assert_eq!(describe(BAT), "Приёмы: fake, multisplit");
        assert!(describe(":: NOT RECOMMENDED\nwinws.exe").starts_with("Не рекомендуется"));
    }

    #[test]
    fn sorts_alt2_before_alt10() {
        let mut ids = vec!["general (ALT10)", "general (ALT2)", "general"];
        ids.sort_by_key(|id| (*id != "general", natural_key(id)));
        assert_eq!(ids, vec!["general", "general (ALT2)", "general (ALT10)"]);
    }

    #[test]
    fn splits_quoted_args() {
        assert_eq!(
            split_args("--wf-tcp=80,443  --hostlist=\"C:\\два слова\\list.txt\""),
            vec!["--wf-tcp=80,443", "--hostlist=C:\\два слова\\list.txt"]
        );
        // Пустые кавычки — это пустой аргумент, а не отсутствие аргумента.
        assert_eq!(split_args("--a=\"\" --b"), vec!["--a=", "--b"]);
    }
}

// ---------- автоподбор ----------

/// Домены, на которых проверяем обход, если пользователь не задал свои.
const DEFAULT_PROBES: &[&str] =
    &["discord.com", "discord.media", "youtube.com", "googlevideo.com", "x.com", "rutracker.org"];

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub id: String,
    pub title: String,
    pub ok: usize,
    pub total: usize,
    pub failed: Vec<String>,
    pub error: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Progress {
    step: usize,
    total: usize,
    title: String,
}

/// Проверяет один домен: важно именно установление TLS-соединения — DPI рвёт
/// его на приветствии, а не отдаёт ошибку HTTP.
async fn probe(domain: &str) -> bool {
    let Ok(client) = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(6))
        .danger_accept_invalid_certs(true)
        .build()
    else {
        return false;
    };
    client.head(format!("https://{domain}/")).send().await.is_ok()
}

async fn probe_all(domains: &[String]) -> (usize, Vec<String>) {
    let results = futures::future::join_all(domains.iter().map(|d| async move { (d.clone(), probe(d).await) })).await;
    let failed: Vec<String> = results.iter().filter(|(_, ok)| !ok).map(|(d, _)| d.clone()).collect();
    (results.len() - failed.len(), failed)
}

/// Перебирает стратегии и проверяет по ним доступность доменов.
/// Возвращает сравнение, включая замер без обхода.
pub async fn autotune(app: &AppHandle, domains: Vec<String>) -> Result<Vec<ProbeResult>> {
    if !winsys::is_elevated() {
        bail!("Нужны права администратора: подбор запускает обход");
    }
    if let Some((pid, owner)) = winsys::find_process("winws.exe") {
        let st = app.state::<AppState>();
        if st.zapret.status().state != "running" {
            bail!("Обход уже запущен другой программой:
{owner}

Выключите обход в ней — иначе подбор ничего не покажет (PID {pid}).");
        }
    }
    let domains: Vec<String> = if domains.is_empty() {
        DEFAULT_PROBES.iter().map(|d| d.to_string()).collect()
    } else {
        domains
    };

    let st = app.state::<AppState>();
    let was_running = st.zapret.status().state == "running";
    if was_running {
        stop(app).await?;
    }
    let exe = cores::ensure(app, "zapret").await?;
    let dir = core_dir(app).context("нет папки ядра")?;
    ensure_lists(app)?;
    // Стратегии применяют обход только к адресам из своих списков. Если
    // проверять сайт, которого там нет, результат всегда совпадёт с замером
    // без обхода — поэтому на время проверки добавляем домены в свой список.
    write_probe_list(app, &domains)?;

    // Кандидаты: сначала замер без обхода, затем все стратегии набора и своя.
    let mut candidates: Vec<(String, String, Option<Vec<String>>)> =
        vec![("none".into(), "Без обхода".into(), None)];
    for s in list_strategies(app) {
        match args_of(app, &s.id) {
            Ok(args) => candidates.push((s.id, s.title, Some(args))),
            Err(_) => continue,
        }
    }
    if !st.settings.lock().unwrap().zapret_custom.trim().is_empty() {
        if let Ok(args) = args_of(app, CUSTOM) {
            candidates.push((CUSTOM.into(), "Своя".into(), Some(args)));
        }
    }

    let total = candidates.len();
    let mut out = Vec::new();
    for (i, (id, title, args)) in candidates.into_iter().enumerate() {
        let _ = app.emit("zapret-autotune", Progress { step: i + 1, total, title: title.clone() });
        let mut running = None;
        let mut error = None;
        if let Some(raw) = &args {
            let exited = Arc::new(AtomicBool::new(false));
            let built = build_args(raw, &dir);
            match child::spawn(
                app,
                Spawn {
                    exe: &exe,
                    args: built.iter().map(|a| a.as_os_str()).collect(),
                    cwd: Some(&dir),
                    source: "dpi",
                    parse: parse_line,
                    exited: exited.clone(),
                },
                |_app, _code| async {},
            )
            .await
            {
                Ok(r) => {
                    tokio::time::sleep(Duration::from_millis(1200)).await;
                    if exited.load(Ordering::SeqCst) {
                        error = Some("не запустилось — проверьте параметры".to_string());
                    }
                    running = Some(r);
                }
                Err(e) => error = Some(format!("{e:#}")),
            }
        }
        let (ok, failed) = if error.is_some() { (0, domains.clone()) } else { probe_all(&domains).await };
        if let Some(r) = running {
            r.stop().await;
            // Драйверу нужно мгновение, чтобы снять фильтры до следующего запуска.
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
        log(app, "info", format!("Подбор: «{title}» — {ok} из {} доменов", domains.len()));
        out.push(ProbeResult { id, title, ok, total: domains.len(), failed, error });
    }

    // Возвращаем список в прежний вид, что бы ни случилось по дороге.
    let _ = rebuild_hostlist(app);
    if was_running {
        start(app).await?;
    }
    Ok(out)
}

/// На время проверки дописывает проверяемые домены в пользовательский список.
fn write_probe_list(app: &AppHandle, domains: &[String]) -> Result<()> {
    let st = app.state::<AppState>();
    let (sets, custom) = {
        let s = st.settings.lock().unwrap();
        (s.zapret_sets.clone(), s.zapret_custom_hosts.clone())
    };
    let mut hosts = crate::hostsets::collect(&sets, &custom);
    for d in domains {
        let d = d.trim().trim_start_matches("*.").to_ascii_lowercase();
        if !d.is_empty() && !hosts.contains(&d) {
            hosts.push(d);
        }
    }
    let Some(path) = user_list_path(app) else { return Ok(()) };
    crate::util::write_atomic(&path, format!("# Проверка KKMProxy\r\n{}", hosts.join("\r\n")))
}
