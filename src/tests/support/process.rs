use chrono::Local;

use crate::app::{self, App};
use crate::model::{
    InfoValue, ProcessEnvironmentEntry, ProcessEnvironmentReport, ProcessInfo, ProcessModuleEntry,
    ProcessModulesReport,
};
use crate::samplers::open_files::{OpenFileEntry, OpenFilesReport};

pub(in crate::tests) fn test_process_info(name: &str, pid: u32) -> ProcessInfo {
    ProcessInfo {
        name: name.to_string(),
        pid,
        start_time: Some(1_700_000_000 + u64::from(pid)),
        ppid: InfoValue::Value("1".to_string()),
        parent_process: InfoValue::Value("parent.exe / PID 1".to_string()),
        arch: InfoValue::Value("x64".to_string()),
        dotnet_version: InfoValue::Value(".NET 10.0.2".to_string()),
        user: InfoValue::Value("test-user".to_string()),
        executable: InfoValue::Value(format!("C:/test/{name}")),
        command_line: InfoValue::Value(name.to_string()),
        file_modified: InfoValue::Value("2026-05-06 00:00:00".to_string()),
        file_size: InfoValue::Value("1,024".to_string()),
        company_name: InfoValue::Value("Test Company".to_string()),
        product_name: InfoValue::Value("Test Product".to_string()),
        product_version: InfoValue::Value("1.0.0".to_string()),
        file_version: InfoValue::Value("1.0.0.1".to_string()),
        workset_bytes: InfoValue::Value("1,024".to_string()),
        workset_private_bytes: InfoValue::Value("512".to_string()),
    }
}

pub(in crate::tests) fn show_process_info_files_tab(app: &mut App) {
    app.open_selected_process_info_dialog().unwrap();
    app.process_info_tab = app::ProcessInfoTab::Files;
    app.process_info_focus = app::ProcessInfoFocus::Content;
}

pub(in crate::tests) fn test_open_files_report(
    name: &str,
    pid: u32,
    file_name: &str,
) -> OpenFilesReport {
    OpenFilesReport {
        pid,
        process_name: name.to_string(),
        total_handles: 1,
        file_handles: 1,
        inaccessible_handles: 0,
        unnamed_file_handles: 0,
        entries: vec![OpenFileEntry {
            path: format!(r"C:\tmp\{file_name}"),
            handle: crate::model::open_files::OpenFileHandle {
                kind: Ok(crate::model::open_files::OpenFileKind::File),
                value: 1,
                access: Ok(1),
                mode: Ok(0x20),
            },
        }],
        error: None,
    }
}

pub(in crate::tests) fn test_process_module_entry(
    file_name: &str,
    company: &str,
) -> ProcessModuleEntry {
    ProcessModuleEntry {
        path: format!(r"C:\Program Files\Test\{file_name}"),
        dll_name: file_name.to_string(),
        directory: r"C:\Program Files\Test".to_string(),
        company_name: InfoValue::Value(company.to_string()),
        product_version: InfoValue::Value("2.0.0".to_string()),
        file_version: InfoValue::Value("2.0.0.1".to_string()),
        modified: InfoValue::Value("2026-08-04 12:34:56".to_string()),
    }
}

pub(in crate::tests) fn test_process_modules_report(
    name: &str,
    pid: u32,
    entries: Vec<ProcessModuleEntry>,
) -> ProcessModulesReport {
    ProcessModulesReport {
        pid,
        process_name: name.to_string(),
        captured_at: Local::now(),
        entries,
    }
}

pub(in crate::tests) fn activate_process_modules_tab(app: &mut App) {
    app.activate_process_info_tab(app::ProcessInfoTab::Dlls)
        .unwrap();
}

pub(in crate::tests) fn test_process_environment_report(
    name: &str,
    pid: u32,
    entries: Vec<ProcessEnvironmentEntry>,
) -> ProcessEnvironmentReport {
    ProcessEnvironmentReport {
        pid,
        process_name: name.to_string(),
        captured_at: Local::now(),
        entries,
        malformed_entries: 0,
    }
}

pub(in crate::tests) fn activate_process_environment_tab(app: &mut App) {
    app.activate_process_info_tab(app::ProcessInfoTab::Environment)
        .unwrap();
}
