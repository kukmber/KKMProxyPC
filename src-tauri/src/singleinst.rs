//! Одна копия программы и открытие окна по ярлыку.
//!
//! Обычный способ — попросить работающую копию показать окно — ломается, когда та
//! запущена от администратора: Windows запрещает программе с обычными правами
//! обращаться к окну программы с правами (защита UIPI), и запрос до неё не доходит.
//! Окно не появляется, и кажется, что ярлык не работает.
//!
//! Поэтому вторая копия просто оставляет файл-флажок в папке данных, а работающая
//! копия его замечает и показывает окно сама. Файлы таким ограничениям не подчинены,
//! поэтому способ работает при любых правах.

use std::path::PathBuf;
use std::time::Duration;
use tauri::AppHandle;

const EXE: &str = "kkmproxypc.exe";

fn flag_path() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(base).join("io.github.kukmber.kkmproxypc").join("show-window.flag"))
}

/// Если программа уже запущена, просит её показать окно и сообщает, что эту копию
/// пора закрыть. Вызывается до создания окна, поэтому ничего лишнего не появится.
pub fn hand_over_to_running() -> bool {
    let Some((pid, _)) = crate::winsys::find_process(EXE) else { return false };
    // Могли попасть в момент перезапуска после обновления: прежняя копия ещё
    // закрывается. Подождём её — иначе программа закрылась бы совсем.
    crate::winsys::wait_for_pid(pid, 3000);
    if crate::winsys::find_process(EXE).is_none() {
        return false;
    }
    if let Some(path) = flag_path() {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&path, b"show");
    }
    true
}

/// Следит за флажком и показывает окно, когда он появится.
pub fn watch(app: &AppHandle) {
    let Some(path) = flag_path() else { return };
    // Флажок мог остаться от прошлого раза — при запуске окно и так на месте.
    let _ = std::fs::remove_file(&path);
    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(400));
        if path.exists() {
            let _ = std::fs::remove_file(&path);
            crate::tray::show_main(&app);
        }
    });
}
