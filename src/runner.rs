//! ExecutionBackend Architecture, Job/Process Manager, and File Format Support Matrix (PRD Sections 4 & 5).
//! Explicitly distinguishes `NATIVE`, `INTERPRETED`, `EMULATED`, `ANALYSIS_ONLY`, and `UNSUPPORTED` states.
//! Never claims a Windows binary executed when the required runtime is absent.

use crate::batch_interp::BatchSession;
use crate::pe::{format_pe_summary, inspect_pe_file};
use crate::reg::{format_reg_report, parse_and_apply_reg, read_reg_file};
use crate::vfs::WinVfs;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    Native,
    Interpreted,
    Emulated,
    AnalysisOnly,
    Unsupported,
}

impl ExecutionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExecutionMode::Native => "NATIVE",
            ExecutionMode::Interpreted => "INTERPRETED",
            ExecutionMode::Emulated => "EMULATED",
            ExecutionMode::AnalysisOnly => "ANALYSIS ONLY",
            ExecutionMode::Unsupported => "UNSUPPORTED",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub file_path: String,
    pub format_label: String,
    pub mode: ExecutionMode,
    pub can_execute: bool,
    pub can_analyze: bool,
    pub reason: String,
    pub launch_plan: String,
}

#[derive(Debug, Clone)]
pub struct JobRecord {
    pub job_id: u64,
    pub state: String, // "queued", "running", "completed", "failed", "cancelled", "unsupported"
    pub mode: String,
    pub command: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub started_at_ms: u64,
    pub finished_at_ms: u64,
    pub cwd_win: String,
    pub title: Option<String>,
}

static NEXT_JOB_ID: AtomicU64 = AtomicU64::new(1);
static ACTIVE_CANCEL_FLAG: Mutex<Option<(u64, Arc<AtomicBool>)>> = Mutex::new(None);
static JOB_HISTORY: Mutex<BTreeMap<u64, JobRecord>> = Mutex::new(BTreeMap::new());

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn has_runtime(bin: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {} >/dev/null 2>&1", bin))
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Probes a file according to the PRD Section 4 File Format Support Matrix and Section 5.1 ExecutionBackend::probe.
pub fn probe_file(file_path: &str, args: &str) -> ProbeResult {
    let vfs = WinVfs::new();
    let _ = vfs.ensure_initialized();
    let real_path = vfs.win_to_android_path(file_path);
    let win_disp = vfs.android_to_win_path(&real_path);

    let ext = real_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let arg_suffix = if args.trim().is_empty() {
        String::new()
    } else {
        format!(" {}", args.trim())
    };

    match ext.as_str() {
        "bat" | "cmd" => ProbeResult {
            file_path: win_disp.clone(),
            format_label: if ext == "cmd" {
                "Windows Command Script (.cmd)".to_string()
            } else {
                "Windows Batch Script (.bat)".to_string()
            },
            mode: ExecutionMode::Interpreted,
            can_execute: true,
            can_analyze: true,
            reason: "Supported by built-in Rust Windows Batch/CMD Interpreter inside isolated Windows VFS.".to_string(),
            launch_plan: format!("wsr::batch_interp(\"{}\"{})", win_disp, arg_suffix),
        },
        "reg" => ProbeResult {
            file_path: win_disp.clone(),
            format_label: "Windows Registry File (.reg)".to_string(),
            mode: ExecutionMode::Interpreted,
            can_execute: true,
            can_analyze: true,
            reason: "Parsed, validated, and applied to the isolated VFS Virtual Registry after confirmation.".to_string(),
            launch_plan: format!("wsr::reg_apply(\"{}\")", win_disp),
        },
        "ps1" => {
            if has_runtime("pwsh") {
                ProbeResult {
                    file_path: win_disp.clone(),
                    format_label: "PowerShell Script (.ps1)".to_string(),
                    mode: ExecutionMode::Interpreted,
                    can_execute: true,
                    can_analyze: true,
                    reason: "PowerShell Core (pwsh) runtime detected on device.".to_string(),
                    launch_plan: format!("pwsh -File \"{}\"{}", real_path.display(), arg_suffix),
                }
            } else {
                ProbeResult {
                    file_path: win_disp.clone(),
                    format_label: "PowerShell Script (.ps1)".to_string(),
                    mode: ExecutionMode::Unsupported,
                    can_execute: false,
                    can_analyze: true,
                    reason: "Cannot run: PowerShell Core (pwsh) runtime is not installed on this Android device. Syntax inspection is available.".to_string(),
                    launch_plan: "UNAVAILABLE (pwsh not found)".to_string(),
                }
            }
        }
        "vbs" => {
            let wine_ok = has_runtime("wine");
            if wine_ok {
                ProbeResult {
                    file_path: win_disp.clone(),
                    format_label: "VBScript (.vbs)".to_string(),
                    mode: ExecutionMode::Emulated,
                    can_execute: true,
                    can_analyze: true,
                    reason: "Wine Windows Script Host (cscript.exe) detected.".to_string(),
                    launch_plan: format!("wine cscript.exe //nologo \"{}\"{}", real_path.display(), arg_suffix),
                }
            } else {
                ProbeResult {
                    file_path: win_disp.clone(),
                    format_label: "VBScript (.vbs)".to_string(),
                    mode: ExecutionMode::AnalysisOnly,
                    can_execute: false,
                    can_analyze: true,
                    reason: "Cannot run: Windows Script Host (Wine cscript.exe) is not installed on this Android device. Source inspection only.".to_string(),
                    launch_plan: "UNAVAILABLE (WSH runtime not installed)".to_string(),
                }
            }
        }
        "exe" | "scr" | "com" | "dll" | "msi" => {
            match inspect_pe_file(&real_path) {
                Ok(pe) => {
                    if ext == "dll" || pe.is_dll {
                        ProbeResult {
                            file_path: win_disp,
                            format_label: format!("PE Dynamic Library ({})", pe.arch),
                            mode: ExecutionMode::AnalysisOnly,
                            can_execute: false,
                            can_analyze: true,
                            reason: pe.support_reason,
                            launch_plan: "DLL cannot be executed directly (PE inspection only)".to_string(),
                        }
                    } else if !pe.is_valid_pe && ext != "msi" {
                        ProbeResult {
                            file_path: win_disp,
                            format_label: pe.arch,
                            mode: ExecutionMode::Unsupported,
                            can_execute: false,
                            can_analyze: true,
                            reason: pe.support_reason,
                            launch_plan: "REJECTED (Invalid or Non-PE Header)".to_string(),
                        }
                    } else if pe.runtime_available {
                        ProbeResult {
                            file_path: win_disp,
                            format_label: format!("Windows PE Executable ({})", pe.arch),
                            mode: ExecutionMode::Emulated,
                            can_execute: true,
                            can_analyze: true,
                            reason: pe.support_reason,
                            launch_plan: format!("box64/wine \"{}\"{}", real_path.display(), arg_suffix),
                        }
                    } else {
                        ProbeResult {
                            file_path: win_disp,
                            format_label: format!("Windows PE ({})", pe.arch),
                            mode: ExecutionMode::AnalysisOnly,
                            can_execute: false,
                            can_analyze: true,
                            reason: pe.support_reason,
                            launch_plan: "UNAVAILABLE (Missing PE emulation backend)".to_string(),
                        }
                    }
                }
                Err(e) => ProbeResult {
                    file_path: win_disp,
                    format_label: "Unreadable Binary".to_string(),
                    mode: ExecutionMode::Unsupported,
                    can_execute: false,
                    can_analyze: false,
                    reason: e,
                    launch_plan: "ERROR".to_string(),
                },
            }
        }
        "rs" => {
            if has_runtime("rustc") {
                ProbeResult {
                    file_path: win_disp,
                    format_label: "Rust Source Code (.rs)".to_string(),
                    mode: ExecutionMode::Native,
                    can_execute: true,
                    can_analyze: true,
                    reason: "Rust compiler (rustc) detected on device.".to_string(),
                    launch_plan: format!("rustc \"{}\" -O -o /tmp/wsr_bin && /tmp/wsr_bin{}", real_path.display(), arg_suffix),
                }
            } else {
                ProbeResult {
                    file_path: win_disp,
                    format_label: "Rust Source Code (.rs)".to_string(),
                    mode: ExecutionMode::AnalysisOnly,
                    can_execute: false,
                    can_analyze: true,
                    reason: "Rust source file: view/edit available. Install rustc toolchain in Termux to compile on-device.".to_string(),
                    launch_plan: "SOURCE VIEW ONLY (rustc not installed)".to_string(),
                }
            }
        }
        "sh" => ProbeResult {
            file_path: win_disp,
            format_label: "POSIX Shell Script (.sh)".to_string(),
            mode: ExecutionMode::Native,
            can_execute: true,
            can_analyze: true,
            reason: "Supported natively by Android POSIX /system/bin/sh.".to_string(),
            launch_plan: format!("sh \"{}\"{}", real_path.display(), arg_suffix),
        },
        "py" => {
            if has_runtime("python3") {
                ProbeResult {
                    file_path: win_disp,
                    format_label: "Python Script (.py)".to_string(),
                    mode: ExecutionMode::Interpreted,
                    can_execute: true,
                    can_analyze: true,
                    reason: "Python 3 interpreter detected.".to_string(),
                    launch_plan: format!("python3 \"{}\"{}", real_path.display(), arg_suffix),
                }
            } else {
                ProbeResult {
                    file_path: win_disp,
                    format_label: "Python Script (.py)".to_string(),
                    mode: ExecutionMode::Unsupported,
                    can_execute: false,
                    can_analyze: true,
                    reason: "Cannot run: python3 interpreter is not installed.".to_string(),
                    launch_plan: "UNAVAILABLE".to_string(),
                }
            }
        }
        "js" | "mjs" => {
            if has_runtime("node") {
                ProbeResult {
                    file_path: win_disp,
                    format_label: "JavaScript (.js)".to_string(),
                    mode: ExecutionMode::Interpreted,
                    can_execute: true,
                    can_analyze: true,
                    reason: "Node.js runtime detected.".to_string(),
                    launch_plan: format!("node \"{}\"{}", real_path.display(), arg_suffix),
                }
            } else {
                ProbeResult {
                    file_path: win_disp,
                    format_label: "JavaScript / JScript (.js)".to_string(),
                    mode: ExecutionMode::AnalysisOnly,
                    can_execute: false,
                    can_analyze: true,
                    reason: "Cannot run: neither WSH nor Node.js runtime is installed. Source view only.".to_string(),
                    launch_plan: "UNAVAILABLE".to_string(),
                }
            }
        }
        "txt" | "md" | "json" | "xml" | "ini" | "cfg" | "log" | "toml" => ProbeResult {
            file_path: win_disp,
            format_label: format!("Text / Config Document (.{})", ext),
            mode: ExecutionMode::AnalysisOnly,
            can_execute: false,
            can_analyze: true,
            reason: "Document/Configuration file: view and edit supported (not an executable script).".to_string(),
            launch_plan: "VIEW / EDIT ONLY".to_string(),
        },
        _ => ProbeResult {
            file_path: win_disp,
            format_label: if ext.is_empty() { "File".to_string() } else { format!(".{} File", ext) },
            mode: ExecutionMode::Unsupported,
            can_execute: false,
            can_analyze: true,
            reason: "Unknown or non-executable file extension. Safe file inspection only.".to_string(),
            launch_plan: "UNSUPPORTED".to_string(),
        },
    }
}

/// Cancels the currently running job (PRD Section 5.1 ExecutionBackend::terminate).
pub fn cancel_active_job() -> Option<u64> {
    if let Ok(guard) = ACTIVE_CANCEL_FLAG.lock() {
        if let Some((jid, ref flag)) = *guard {
            flag.store(true, Ordering::SeqCst);
            return Some(jid);
        }
    }
    None
}

/// Executes a script file or interactive command through the Job Manager (PRD Section 5.2).
pub fn execute_job(
    file_or_cmd: &str,
    is_script_file: bool,
    args_str: &str,
    cwd_win: &str,
) -> JobRecord {
    let vfs = WinVfs::new();
    let _ = vfs.ensure_initialized();
    let job_id = NEXT_JOB_ID.fetch_add(1, Ordering::SeqCst);
    let started_at_ms = now_ms();
    let cancel_flag = Arc::new(AtomicBool::new(false));
    if let Ok(mut guard) = ACTIVE_CANCEL_FLAG.lock() {
        *guard = Some((job_id, cancel_flag.clone()));
    }

    let arg_vec: Vec<String> = if args_str.trim().is_empty() {
        Vec::new()
    } else {
        args_str.split_whitespace().map(|s| s.to_string()).collect()
    };

    let record = if is_script_file {
        let probe = probe_file(file_or_cmd, args_str);
        let real_path = vfs.win_to_android_path(file_or_cmd);
        if !real_path.exists() {
            JobRecord {
                job_id,
                state: "failed".to_string(),
                mode: "UNSUPPORTED".to_string(),
                command: file_or_cmd.to_string(),
                stdout: String::new(),
                stderr: format!("The system cannot find the file specified: {}", file_or_cmd),
                exit_code: 2,
                started_at_ms,
                finished_at_ms: now_ms(),
                cwd_win: cwd_win.to_string(),
                title: None,
            }
        } else if !probe.can_execute {
            JobRecord {
                job_id,
                state: "unsupported".to_string(),
                mode: probe.mode.as_str().to_string(),
                command: file_or_cmd.to_string(),
                stdout: String::new(),
                stderr: probe.reason,
                exit_code: 126,
                started_at_ms,
                finished_at_ms: now_ms(),
                cwd_win: cwd_win.to_string(),
                title: None,
            }
        } else {
            let ext = real_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            match ext.as_str() {
                "bat" | "cmd" => {
                    match fs::read_to_string(&real_path) {
                        Ok(src) => {
                            let script_parent_win = real_path
                                .parent()
                                .map(|p| vfs.android_to_win_path(p))
                                .unwrap_or_else(|| cwd_win.to_string());
                            let mut session = BatchSession::new(
                                vfs.clone(),
                                &script_parent_win,
                                &probe.file_path,
                                arg_vec,
                            );
                            session.cancel_flag = Some(cancel_flag);
                            let out = session.execute_script(&src);
                            JobRecord {
                                job_id,
                                state: if out.cancelled {
                                    "cancelled".to_string()
                                } else if out.exit_code == 0 {
                                    "completed".to_string()
                                } else {
                                    "failed".to_string()
                                },
                                mode: probe.mode.as_str().to_string(),
                                command: probe.launch_plan,
                                stdout: out.stdout,
                                stderr: out.stderr,
                                exit_code: out.exit_code,
                                started_at_ms,
                                finished_at_ms: now_ms(),
                                cwd_win: out.final_cwd_win,
                                title: out.title,
                            }
                        }
                        Err(e) => JobRecord {
                            job_id,
                            state: "failed".to_string(),
                            mode: probe.mode.as_str().to_string(),
                            command: file_or_cmd.to_string(),
                            stdout: String::new(),
                            stderr: format!("Failed to read batch file: {}", e),
                            exit_code: 1,
                            started_at_ms,
                            finished_at_ms: now_ms(),
                            cwd_win: cwd_win.to_string(),
                            title: None,
                        },
                    }
                }
                "reg" => match read_reg_file(&real_path) {
                    Ok(src) => {
                        let rep = parse_and_apply_reg(&src, &probe.file_path, true);
                        JobRecord {
                            job_id,
                            state: "completed".to_string(),
                            mode: probe.mode.as_str().to_string(),
                            command: probe.launch_plan,
                            stdout: format_reg_report(&rep),
                            stderr: String::new(),
                            exit_code: 0,
                            started_at_ms,
                            finished_at_ms: now_ms(),
                            cwd_win: cwd_win.to_string(),
                            title: None,
                        }
                    }
                    Err(e) => JobRecord {
                        job_id,
                        state: "failed".to_string(),
                        mode: probe.mode.as_str().to_string(),
                        command: file_or_cmd.to_string(),
                        stdout: String::new(),
                        stderr: e,
                        exit_code: 1,
                        started_at_ms,
                        finished_at_ms: now_ms(),
                        cwd_win: cwd_win.to_string(),
                        title: None,
                    },
                },
                _ => {
                    let (stdout, stderr, code) = run_posix_command(&probe.launch_plan, &vfs, cwd_win);
                    JobRecord {
                        job_id,
                        state: if code == 0 { "completed".to_string() } else { "failed".to_string() },
                        mode: probe.mode.as_str().to_string(),
                        command: probe.launch_plan,
                        stdout,
                        stderr,
                        exit_code: code,
                        started_at_ms,
                        finished_at_ms: now_ms(),
                        cwd_win: cwd_win.to_string(),
                        title: None,
                    }
                }
            }
        }
    } else {
        // Interactive command line in Command Prompt: execute through BatchSession first
        let mut session = BatchSession::new(vfs.clone(), cwd_win, "cmd.exe", Vec::new());
        session.cancel_flag = Some(cancel_flag);
        let out = session.execute_script(file_or_cmd);
        JobRecord {
            job_id,
            state: if out.cancelled {
                "cancelled".to_string()
            } else if out.exit_code == 0 {
                "completed".to_string()
            } else {
                "failed".to_string()
            },
            mode: "INTERPRETED".to_string(),
            command: file_or_cmd.to_string(),
            stdout: out.stdout,
            stderr: out.stderr,
            exit_code: out.exit_code,
            started_at_ms,
            finished_at_ms: now_ms(),
            cwd_win: out.final_cwd_win,
            title: out.title,
        }
    };

    if let Ok(mut guard) = ACTIVE_CANCEL_FLAG.lock() {
        *guard = None;
    }
    if let Ok(mut hist) = JOB_HISTORY.lock() {
        if hist.len() >= 50 {
            if let Some(&first_k) = hist.keys().next() {
                hist.remove(&first_k);
            }
        }
        hist.insert(job_id, record.clone());
    }
    record
}

fn run_posix_command(cmd: &str, vfs: &WinVfs, cwd_win: &str) -> (String, String, i32) {
    let sh_bin = if Path::new("/system/bin/sh").exists() && !Path::new("/bin/sh").exists() {
        "/system/bin/sh"
    } else {
        "/bin/sh"
    };
    let work_dir = vfs.win_to_android_path(cwd_win);
    let mut command = Command::new(sh_bin);
    command.arg("-c").arg(cmd);
    if work_dir.exists() && work_dir.is_dir() {
        command.current_dir(&work_dir);
    }
    match command.output() {
        Ok(out) => (
            String::from_utf8_lossy(&out.stdout).trim_end().to_string(),
            String::from_utf8_lossy(&out.stderr).trim_end().to_string(),
            out.status.code().unwrap_or(1),
        ),
        Err(e) => (String::new(), format!("Failed to spawn process: {}", e), 1),
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
    pub format_label: String,
    pub mode: String,
    pub can_execute: bool,
    pub can_analyze: bool,
}

pub fn list_directory_native(dir_path: &str) -> Result<Vec<FileEntry>, String> {
    let vfs = WinVfs::new();
    let _ = vfs.ensure_initialized();
    let resolved = vfs.win_to_android_path(dir_path);
    if !resolved.exists() {
        return Err(format!("Directory does not exist: {} (mapped to {})", dir_path, resolved.display()));
    }
    let rd = fs::read_dir(&resolved).map_err(|e| format!("Permission denied or unreadable directory {}: {}", dir_path, e))?;
    let mut entries = Vec::new();
    for item in rd.flatten() {
        let name = item.file_name().to_string_lossy().into_owned();
        let full_path_buf = item.path();
        let full_path = full_path_buf.to_string_lossy().into_owned();
        let win_path = vfs.android_to_win_path(&full_path_buf);
        let meta = item.metadata().ok();
        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);

        let (format_label, mode, can_execute, can_analyze) = if is_dir {
            ("File Folder".to_string(), "FOLDER".to_string(), false, false)
        } else {
            let p = probe_file(&win_path, "");
            (p.format_label, p.mode.as_str().to_string(), p.can_execute, p.can_analyze)
        };

        entries.push(FileEntry {
            name,
            path: full_path,
            win_path,
            is_dir,
            size,
            format_label,
            mode,
            can_execute,
            can_analyze,
        });
    }
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

pub fn run_environment_check() -> String {
    let vfs = WinVfs::new();
    let _ = vfs.ensure_initialized();
    let check_rt = |name: &str, bin: &str| -> String {
        if has_runtime(bin) {
            format!("  [INSTALLED]     {}", name)
        } else {
            format!("  [NOT INSTALLED] {}", name)
        }
    };
    let lines = vec![
        "=== Windows Script Runner Capabilities & VFS Status (v4.0) ===".to_string(),
        "VFS Backing Storage:".to_string(),
        format!("  {}", vfs.describe_drive_backing("C:\\")),
        format!("  {}", vfs.describe_drive_backing("D:\\")),
        format!("  {}", vfs.describe_drive_backing("Z:\\")),
        "\nBuilt-In Rust Execution Engines (Always Ready):".to_string(),
        "  [INTERPRETED]   .bat / .cmd  -> Native Rust Batch/CMD Interpreter".to_string(),
        "  [INTERPRETED]   .reg         -> Native Rust Registry Parser & Isolated Virtual Registry".to_string(),
        "  [ANALYSIS ONLY] .exe / .dll  -> Native Rust PE32/PE32+ Header, Section & Import Analyzer".to_string(),
        "\nOptional External Runtimes on Device:".to_string(),
        check_rt("PowerShell Core (.ps1 -> pwsh)", "pwsh"),
        check_rt("Wine Compatibility Layer (.exe/.vbs/.msi -> wine)", "wine"),
        check_rt("Box64 x86_64 Emulator (.exe -> box64)", "box64"),
        check_rt("Mono CLR Runtime (.NET .exe -> mono)", "mono"),
        check_rt("Rust Compiler (.rs -> rustc)", "rustc"),
        check_rt("Python 3 (.py -> python3)", "python3"),
        check_rt("Node.js (.js -> node)", "node"),
    ];
    lines.join("\n")
}
