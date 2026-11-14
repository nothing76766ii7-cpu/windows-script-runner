//! Core library & Android JNI shared library (`libwsr_jni.so`) for Windows Script Runner (PRD v4.0).

pub mod batch_interp;
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
            if !COUNT! EQU 15 (\r\n\
              echo Branch OK\r\n\
            )\r\n\
            for %%x in (alpha beta) do echo Item: %%x\r\n\
            goto :finish\r\n\
            echo Should be skipped\r\n\
            :finish\r\n\
            exit /b 0\r\n";
        let out = session.execute_script(script);
        assert_eq!(out.exit_code, 0);
        assert_eq!(out.title.as_deref(), Some("Custom Title"));
        assert!(out.stdout.contains("Count is 15 and arg1 is arg_one"));
        assert!(out.stdout.contains("Item: alpha"));
        assert!(out.stdout.contains("Item: beta"));
        assert!(!out.stdout.contains("Should be skipped"));
        assert!(!out.stdout.contains("\x1b]0;"));
    }

    #[test]
    fn test_pe_rejects_text_exe_and_parses_real_pe() {
        let vfs = WinVfs::new();
        let _ = vfs.ensure_initialized();
        let fake_exe = vfs.drive_c.join("Temp/fake_text.exe");
        let _ = fs::write(&fake_exe, "This is plain text, not an MZ/PE binary.");
        let analysis = inspect_pe_file(&fake_exe).unwrap();
        assert!(!analysis.is_valid_pe);
        assert_eq!(analysis.status, "NON_PE");

        let probe = probe_file("C:\\Temp\\fake_text.exe", "");
        assert!(!probe.can_execute);
        assert_eq!(probe.mode, ExecutionMode::Unsupported);
    }

    #[test]
    fn test_virtual_registry_isolation() {
        let reg = "Windows Registry Editor Version 5.00\n\
            [HKEY_CURRENT_USER\\Environment]\n\
            \"MY_SDK\"=\"C:\\\\Sdk\"\n\
            [HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run]\n\
            \"AutorunApp\"=\"C:\\\\app.exe\"\n";
        let rep = parse_and_apply_reg(reg, "C:\\Users\\Admin\\Desktop\\test.reg", false);
        assert!(!rep.applied_mode);
        assert_eq!(rep.env_vars.len(), 1);
        assert_eq!(rep.warnings.len(), 1);
    }
}
