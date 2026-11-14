//! Core library & Android JNI shared library (`libwsr_jni.so`) for Windows Script Runner (Rust Edition v3.0).

pub mod mcp;
pub mod pe;
pub mod reg;
pub mod runner;
pub mod server;
pub mod transpiler;
pub mod vfs;

#[cfg(test)]
mod tests {
    use super::pe::inspect_pe_file;
    use super::reg::parse_and_apply_reg;
    use super::runner::{detect_strategy, ScriptStrategy};
    use super::transpiler::transpile_bat;
    use super::vfs::WinVfs;

    #[test]
    fn test_windows_vfs_and_bat_transpilation() {
        let vfs = WinVfs::new();
        let p = vfs.win_to_android_path("C:\\Windows\\System32\\cmd.exe");
        assert!(p.to_string_lossy().ends_with("Windows/System32/cmd.exe"));

        let bat = "@echo off\r\nC:\r\ncd \\Users\\Admin\\Desktop\r\nset /a TOTAL=10+20\r\necho Result %TOTAL% > C:\\Temp\\out.txt\r\nexit /b 0\r\n";
        let res = transpile_bat(bat);
        assert!(res.script.contains("cd \"${WSR_DRIVE_C}\""));
        assert!(res.script.contains("export TOTAL=$(( 10+20 ))"));
        assert!(res.script.contains("${WSR_DRIVE_C}/Temp/out.txt"));
    }

    #[test]
    fn test_exe_and_msi_strategies() {
        assert_eq!(detect_strategy("C:\\Program Files\\app.exe"), Some(ScriptStrategy::Exe));
        assert_eq!(detect_strategy("installer.msi"), Some(ScriptStrategy::Msi));
        assert_eq!(detect_strategy("deploy.cmd"), Some(ScriptStrategy::Bat));
    }

    #[test]
    fn test_reg_parser() {
        let reg = "Windows Registry Editor Version 5.00\n[HKEY_CURRENT_USER\\Environment]\n\"SDK_DIR\"=\"C:\\\\Android\\\\sdk\"\n";
        let rep = parse_and_apply_reg(reg, "sdk.reg", false);
        assert_eq!(rep.applied.len(), 1);
    }
}
