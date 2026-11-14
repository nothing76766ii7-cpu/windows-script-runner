//! Script strategy detection, Windows VFS directory listing, PE `.exe`/`.msi` execution, and shell runner.

use crate::pe::{build_exe_command, format_pe_summary, inspect_pe_file};
use crate::reg::{format_reg_report, parse_and_apply_reg, read_reg_file};
use crate::transpiler::transpile_bat;
use crate::vfs::WinVfs;
use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptStrategy {
    Bat,
    Ps1,
    Vbs,
    Reg,
    Py,
    Js,
    Sh,
    Rs,
    Exe,
    Msi,
}

impl ScriptStrategy {
    pub fn as_str(&self) -> &'static str {
        match self {
            ScriptStrategy::Bat => "bat",
            ScriptStrategy::Ps1 => "ps1",
            ScriptStrategy::Vbs => "vbs",
            ScriptStrategy::Reg => "reg",
            ScriptStrategy::Py => "py",
            ScriptStrategy::Js => "js",
            ScriptStrategy::Sh => "sh",
            ScriptStrategy::Rs => "rs",
            ScriptStrategy::Exe => "exe",
            ScriptStrategy::Msi => "msi",
        }
    }

    pub fn is_runnable(&self) -> bool {
        true
    }
}

pub fn detect_strategy(file_path: &str) -> Option<ScriptStrategy> {
    let clean = file_path.replace('\\', "/");
    let ext = Path::new(&clean)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())?;

    match ext.as_str() {
        "bat" | "cmd" => Some(ScriptStrategy::Bat),
        "ps1" => Some(ScriptStrategy::Ps1),
        "vbs" => Some(ScriptStrategy::Vbs),
        "reg" => Some(ScriptStrategy::Reg),
        "py" => Some(ScriptStrategy::Py),
        "js" | "mjs" => Some(ScriptStrategy::Js),
        "sh" => Some(ScriptStrategy::Sh),
        "rs" => Some(ScriptStrategy::Rs),
        "exe" => Some(ScriptStrategy::Exe),
        "msi" => Some(ScriptStrategy::Msi),
        _ => None,
    }
}

pub fn build_command(file_path: &str, strategy: ScriptStrategy, args: &str) -> String {
    let vfs = WinVfs::new();
    let _ = vfs.ensure_initialized();
    let real_path_buf = vfs.win_to_android_path(file_path);
    let real_path = real_path_buf.to_string_lossy().into_owned();

    let suffix = if args.trim().is_empty() {
        String::new()
    } else {
        format!(" {}", args.trim())
    };
    let sh_path = replace_extension(&real_path, "sh");
    let bin_out = "/tmp/wsr_rust_script_bin";

    match strategy {
        ScriptStrategy::Bat => format!(
            "~/.antigem_tools/bin/bat2sh \"{}\" && chmod +x \"{}\" && sh \"{}\"{}",
            real_path, sh_path, sh_path, suffix
        ),
        ScriptStrategy::Ps1 => format!("pwsh -File \"{}\"{}", real_path, suffix),
        ScriptStrategy::Vbs => format!(
            "proot-distro login ubuntu -- wine cscript.exe //nologo \"{}\"{}",
            real_path, suffix
        ),
        ScriptStrategy::Reg => format!("~/.antigem_tools/bin/reg_parser \"{}\"", real_path),
        ScriptStrategy::Py => format!("python3 \"{}\"{}", real_path, suffix),
        ScriptStrategy::Js => format!("node \"{}\"{}", real_path, suffix),
        ScriptStrategy::Sh => format!("chmod +x \"{}\" && sh \"{}\"{}", real_path, real_path, suffix),
        ScriptStrategy::Rs => format!(
            "rustc \"{}\" -O -o \"{}\" && \"{}\"{}",
            real_path, bin_out, bin_out, suffix
        ),
        ScriptStrategy::Exe => build_exe_command(&real_path, args, false),
        ScriptStrategy::Msi => build_exe_command(&real_path, args, true),
    }
}

/// Executes a script or shell command directly in the native Rust backend (used by the standalone Android APK and HTTP server).
pub fn execute_direct(file_or_cmd: &str, is_script_file: bool, args: &str) -> (String, i32) {
    let vfs = WinVfs::new();
    let _ = vfs.ensure_initialized();

    if is_script_file {
        let real_path = vfs.win_to_android_path(file_or_cmd);
        let real_str = real_path.to_string_lossy().into_owned();
        if let Some(strat) = detect_strategy(&real_str) {
            match strat {
                ScriptStrategy::Bat => {
                    if let Ok(src) = fs::read_to_string(&real_path) {
                        let res = transpile_bat(&src);
                        let sh_path = replace_extension(&real_str, "sh");
                        let _ = fs::write(&sh_path, &res.script);
                        let run_cmd = if args.trim().is_empty() {
                            format!("sh \"{}\"", sh_path)
                        } else {
                            format!("sh \"{}\" {}", sh_path, args.trim())
                        };
                        return run_shell_with_vfs(&run_cmd, &vfs);
                    }
                }
                ScriptStrategy::Reg => {
                    if let Ok(src) = read_reg_file(&real_path) {
                        let rep = parse_and_apply_reg(&src, &real_str, true);
                        return (format_reg_report(&rep), 0);
                    }
                }
                ScriptStrategy::Exe => {
                    let mut out_log = String::new();
                    if let Ok(pe_info) = inspect_pe_file(&real_path) {
                        out_log.push_str(&format_pe_summary(&real_path, &pe_info));
                        out_log.push_str("\n\n");
                    }
                    let cmd = build_exe_command(&real_str, args, false);
                    let (sh_out, code) = run_shell_with_vfs(&cmd, &vfs);
                    out_log.push_str(&sh_out);
                    return (out_log, code);
                }
                _ => {
                    let cmd = build_command(&real_str, strat, args);
                    return run_shell_with_vfs(&cmd, &vfs);
                }
            }
        }
    }

    // Direct terminal command execution with Windows VFS environment variables
    let full_cmd = format!("{}\n{}", vfs.posix_env_preamble(), file_or_cmd);
    run_shell_with_vfs(&full_cmd, &vfs)
}

fn run_shell_with_vfs(cmd: &str, vfs: &WinVfs) -> (String, i32) {
    let sh_bin = if Path::new("/system/bin/sh").exists() && !Path::new("/bin/sh").exists() {
        "/system/bin/sh"
    } else {
        "/bin/sh"
    };

    let output = Command::new(sh_bin)
        .arg("-c")
        .arg(cmd)
        .env("WSR_DRIVE_C", &vfs.drive_c)
        .env("WSR_DRIVE_D", &vfs.drive_d)
        .env("WSR_DRIVE_Z", "/")
        .env("USERPROFILE", vfs.drive_c.join("Users/Admin"))
        .env("SystemRoot", vfs.drive_c.join("Windows"))
        .env("TEMP", vfs.drive_c.join("Users/Admin/AppData/Local/Temp"))
        .output();

    match output {
        Ok(out) => {
            let mut combined = String::from_utf8_lossy(&out.stdout).into_owned();
            let err = String::from_utf8_lossy(&out.stderr);
            if !err.trim().is_empty() {
                if !combined.is_empty() && !combined.ends_with('\n') {
                    combined.push('\n');
                }
                combined.push_str(&err);
            }
            let code = out.status.code().unwrap_or(0);
            (combined, code)
        }
        Err(e) => (format!("[ERROR] Shell execution failed: {}", e), 1),
    }
}

pub fn replace_extension(file_path: &str, new_ext: &str) -> String {
    let p = Path::new(file_path);
    p.with_extension(new_ext).to_string_lossy().into_owned()
}

pub fn expand_tilde(path: &str) -> String {
    let vfs = WinVfs::new();
    let _ = vfs.ensure_initialized();
    vfs.win_to_android_path(path).to_string_lossy().into_owned()
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub win_path: String,
    pub is_dir: bool,
    pub size: u64,
    pub strategy: Option<ScriptStrategy>,
    pub pe_arch: Option<String>,
}

pub fn list_directory_native(dir_path: &str) -> Result<Vec<FileEntry>, String> {
    let vfs = WinVfs::new();
    let _ = vfs.ensure_initialized();
    let resolved = vfs.win_to_android_path(dir_path);
    let rd = fs::read_dir(&resolved).map_err(|e| format!("Cannot read {}: {}", resolved.display(), e))?;
    let mut entries = Vec::new();
    for item in rd.flatten() {
        let name = item.file_name().to_string_lossy().into_owned();
        let full_path_buf = item.path();
        let full_path = full_path_buf.to_string_lossy().into_owned();
        let win_path = vfs.android_to_win_path(&full_path_buf);
        let meta = item.metadata().ok();
        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        let strategy = if is_dir { None } else { detect_strategy(&name) };
        let pe_arch = if matches!(strategy, Some(ScriptStrategy::Exe)) {
            inspect_pe_file(&full_path_buf).ok().map(|i| i.arch)
        } else {
            None
        };
        entries.push(FileEntry {
            name,
            path: full_path,
            win_path,
            is_dir,
            size,
            strategy,
            pe_arch,
        });
    }
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

pub fn check_binary_version(bin: &str, arg: &str) -> String {
    match Command::new(bin).arg(arg).output() {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            let first = text.lines().next().unwrap_or("installed").trim();
            format!("{}: OK ({})", bin, first)
        }
        _ => format!("{}: not installed", bin),
    }
}

pub fn run_environment_check() -> String {
    let vfs = WinVfs::new();
    let _ = vfs.ensure_initialized();
    let mut lines = vec![
        "=== Windows Script Runner Environment & VFS Check (Rust v3.0) ===".to_string(),
        format!("Drive C: (Windows VFS) -> {}", vfs.drive_c.display()),
        format!("Drive D: (Android SD)  -> {}", vfs.drive_d.display()),
        format!("Drive Z: (Linux Root)  -> {}", vfs.drive_z.display()),
        check_binary_version("rustc", "--version"),
        check_binary_version("cargo", "--version"),
        check_binary_version("sh", "--version"),
        check_binary_version("box64", "--version"),
        check_binary_version("wine", "--version"),
        check_binary_version("mono", "--version"),
        check_binary_version("pwsh", "--version"),
        check_binary_version("python3", "--version"),
        check_binary_version("node", "--version"),
    ];
    let home = env::var("HOME").unwrap_or_else(|_| "/data/data/com.vanta.wsr/files/home".to_string());
    let tools_dir = format!("{}/.antigem_tools/bin", home);
    let bat2sh_ok = Path::new(&tools_dir).join("bat2sh").exists();
    let reg_ok = Path::new(&tools_dir).join("reg_parser").exists();
    lines.push(format!(
        "Rust Helper Binaries (~/.antigem_tools/bin): bat2sh={}, reg_parser={}",
        if bat2sh_ok { "READY" } else { "EMBEDDED_ENGINE_ACTIVE" },
        if reg_ok { "READY" } else { "EMBEDDED_ENGINE_ACTIVE" }
    ));
    lines.join("\n")
}
