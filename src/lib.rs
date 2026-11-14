//! Core library & Android JNI shared library (`libwsr_jni.so`) for Windows Script Runner (PRD v5.0).

pub mod batch_interp;
pub mod gui_session;
pub mod mcp;
pub mod pe;
pub mod reg;
pub mod runner;
pub mod server;
pub mod transpiler;
pub mod vfs;

#[cfg(test)]
mod tests {
    use super::batch_interp::BatchSession;
    use super::gui_session::launch_pe_session;
    use super::pe::inspect_pe_file;
    use super::reg::parse_and_apply_reg;
    use super::runner::{probe_file, ExecutionMode};
    use super::vfs::WinVfs;
    use std::fs;

    #[test]
    fn test_batch_interpreter_mvp() {
        let vfs = WinVfs::new();
        let _ = vfs.ensure_initialized();
        let mut session = BatchSession::new(
            vfs,
            "C:\\Users\\Admin\\Desktop",
            "test.bat",
            vec!["arg_one".to_string()],
        );
        let script = "@echo off\r\n\
            title Custom Title\r\n\
            setlocal enabledelayedexpansion\r\n\
            set /a COUNT=10+5\r\n\
            echo Count is !COUNT! and arg1 is %1\r\n\
            for %%x in (alpha beta) do echo Item: %%x\r\n\
            goto :finish\r\n\
            echo Should be skipped\r\n\
            :finish\r\n\
            exit /b 0\r\n";
        let out = session.execute_script(script);
        assert_eq!(out.exit_code, 0);
        assert_eq!(out.title.as_deref(), Some("Custom Title"));
        assert!(out.stdout.contains("Count is 15 and arg1 is arg_one"));
        assert!(!out.stdout.contains("Should be skipped"));
    }

    #[test]
    fn test_downloads_not_populated_with_fake_files() {
        let vfs = WinVfs::new();
        let _ = vfs.ensure_initialized();
        let status = vfs.inspect_storage_source("C:\\Users\\Admin\\Downloads");
        assert!(status.is_real_android);
        // Without an explicit SAF mount, Downloads reports is_authorized = false so the UI shows 'Connect Downloads'
        assert!(!status.is_authorized);
    }

    #[test]
    fn test_gui_session_never_fakes_window_without_runtime() {
        let vfs = WinVfs::new();
        let _ = vfs.ensure_initialized();
        let fake_exe = vfs.drive_c.join("Temp/non_pe.exe");
        let _ = fs::write(&fake_exe, "Not a PE binary");
        let rec = launch_pe_session("C:\\Temp\\non_pe.exe", "", 5);
        assert_eq!(rec.state, "UNSUPPORTED");
        assert!(rec.windows.is_empty());

        let analysis = inspect_pe_file(&fake_exe).unwrap();
        assert!(!analysis.is_valid_pe);
        let probe = probe_file("C:\\Temp\\non_pe.exe", "");
        assert_eq!(probe.mode, ExecutionMode::Unsupported);
    }

    #[test]
    fn test_virtual_registry_isolation() {
        let reg = "Windows Registry Editor Version 5.00\n\
            [HKEY_CURRENT_USER\\Environment]\n\
            \"MY_SDK\"=\"C:\\\\Sdk\"\n";
        let rep = parse_and_apply_reg(reg, "C:\\Users\\Admin\\Desktop\\test.reg", false);
        assert!(!rep.applied_mode);
        assert_eq!(rep.env_vars.len(), 1);
    }
}
