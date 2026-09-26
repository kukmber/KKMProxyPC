//! Небольшие обёртки над WinAPI: права администратора, перезапуск с повышением,
//! Job Object (ядра умирают вместе с приложением), версия Windows.

use anyhow::{bail, Result};
use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::sync::OnceLock;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, WaitForSingleObject, PROCESS_SYNCHRONIZE,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn wide(s: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(std::iter::once(0)).collect()
}

pub fn is_elevated() -> bool {
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut el = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut el as *mut _ as *mut c_void,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        CloseHandle(token);
        ok != 0 && el.TokenIsElevated != 0
    }
}

/// Запускает копию приложения с правами администратора. Новая копия ждёт,
/// пока текущая завершится (`--wait-pid`), иначе её остановил бы single-instance.
pub fn relaunch_elevated(extra_args: &[&str]) -> Result<()> {
    let exe = std::env::current_exe()?;
    let mut params = format!("--wait-pid {}", std::process::id());
    for a in extra_args {
        params.push(' ');
        params.push_str(a);
    }
    let (verb, file, params) = (wide("runas"), wide(&exe), wide(&params));
    let r = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            params.as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if (r as isize) <= 32 {
        bail!("Запуск от имени администратора отменён");
    }
    Ok(())
}

pub fn wait_for_pid(pid: u32, timeout_ms: u32) {
    unsafe {
        let h = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if !h.is_null() {
            WaitForSingleObject(h, timeout_ms);
            CloseHandle(h);
        }
    }
}

static JOB: OnceLock<usize> = OnceLock::new();

/// Привязывает процесс к Job Object с KILL_ON_JOB_CLOSE: если приложение упадёт
/// или его завершат из диспетчера задач, Windows сама остановит ядро.
pub fn attach_to_job(process: HANDLE) {
    let job = *JOB.get_or_init(|| unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        job as usize
    });
    unsafe {
        AssignProcessToJobObject(job as HANDLE, process);
    }
}

/// Номер сборки Windows (Mica появилась в 22000 — Windows 11).
pub fn windows_build() -> u32 {
    use windows_sys::Wdk::System::SystemServices::RtlGetVersion;
    use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW;
    unsafe {
        let mut v: OSVERSIONINFOW = std::mem::zeroed();
        v.dwOSVersionInfoSize = std::mem::size_of::<OSVERSIONINFOW>() as u32;
        if RtlGetVersion(&mut v) == 0 {
            v.dwBuildNumber
        } else {
            0
        }
    }
}

/// Сколько установленных входящих соединений держит процесс `pid` на порту `port`.
/// Своё число подключений tg-ws-proxy наружу не отдаёт, поэтому спрашиваем Windows.
pub fn established_connections(pid: u32, port: u16) -> usize {
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_ALL,
    };
    use windows_sys::Win32::Networking::WinSock::AF_INET;
    const ESTABLISHED: u32 = 5;

    unsafe {
        let mut size: u32 = 0;
        GetExtendedTcpTable(std::ptr::null_mut(), &mut size, 0, AF_INET as u32, TCP_TABLE_OWNER_PID_ALL, 0);
        if size == 0 {
            return 0;
        }
        let mut buf = vec![0u8; size as usize];
        if GetExtendedTcpTable(
            buf.as_mut_ptr() as *mut c_void,
            &mut size,
            0,
            AF_INET as u32,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        ) != 0
        {
            return 0;
        }
        let table = &*(buf.as_ptr() as *const MIB_TCPTABLE_OWNER_PID);
        let rows = std::slice::from_raw_parts(table.table.as_ptr() as *const MIB_TCPROW_OWNER_PID, table.dwNumEntries as usize);
        rows.iter()
            .filter(|r| {
                // Порт в таблице лежит в сетевом порядке байтов.
                let local = u16::from_be((r.dwLocalPort & 0xffff) as u16);
                r.dwOwningPid == pid && r.dwState == ESTABLISHED && local == port
            })
            .count()
    }
}

/// Ищет чужой запущенный процесс по имени. Возвращает его pid и путь к файлу —
/// по пути сразу видно, какая программа его держит.
pub fn find_process(exe_name: &str) -> Option<(u32, String)> {
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            return None;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let me = std::process::id();
        let mut found = None;
        if Process32FirstW(snap, &mut entry) != 0 {
            loop {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
                if name.eq_ignore_ascii_case(exe_name)
                    && entry.th32ProcessID != me
                    && entry.th32ParentProcessID != me
                {
                    found = Some(entry.th32ProcessID);
                    break;
                }
                if Process32NextW(snap, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
        let pid = found?;
        Some((pid, process_path(pid).unwrap_or_else(|| exe_name.to_string())))
    }
}

/// Полный путь к файлу процесса.
fn process_path(pid: u32) -> Option<String> {
    use windows_sys::Win32::System::Threading::{QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(h);
        (ok != 0).then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// Имена запущенных программ — чтобы правило можно было выбрать из списка,
/// а не вспоминать, как называется файл.
pub fn running_processes() -> Vec<String> {
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    let mut out: Vec<String> = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            return out;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snap, &mut entry) != 0 {
            loop {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
                if !name.is_empty() && !out.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
                    out.push(name);
                }
                if Process32NextW(snap, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
    }
    out.sort_by_key(|n| n.to_lowercase());
    out
}
