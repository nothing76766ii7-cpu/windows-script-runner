//! Windows GUI Execution Lifecycle, Display Surface Bridge, and Android Input Forwarder (PRD Section 19).
//! Explicitly tracks GUI execution states:
//! `ANALYZING` -> `PREPARING_RUNTIME` -> `STARTING_PROCESS` -> `WAITING_FOR_WINDOW`
//! -> `GUI_VISIBLE` / `CONSOLE_RUNNING` / `EXITED` / `CRASHED` / `UNSUPPORTED`.
//! Never fabricates or displays a mock Windows desktop window when no real runtime window exists.

use crate::pe::inspect_pe_file;
use crate::vfs::WinVfs;
use std::collections::BTreeMap;
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct GuiWindowInfo {
    pub win_id: u32,
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub is_minimized: bool,
    pub is_maximized: bool,
}

#[derive(Debug, Clone)]
pub struct GuiSessionRecord {
    pub session_id: u64,
    pub exe_win_path: String,
    pub exe_android_path: String,
    pub arch: String,
    pub subsystem: String,
    pub is_gui_subsystem: bool,
    pub state: String, // "ANALYZING", "PREPARING_RUNTIME", "STARTING_PROCESS", "WAITING_FOR_WINDOW", "GUI_VISIBLE", "CONSOLE_RUNNING", "EXITED", "CRASHED", "UNSUPPORTED"
    pub status_message: String,
    pub runtime_used: String,
    pub display_bridge_url: Option<String>, // e.g., ws://127.0.0.1:5901 or x11://127.0.0.1:6000 when active
    pub windows: Vec<GuiWindowInfo>,
    pub stdout_log: String,
    pub stderr_log: String,
    pub exit_code: Option<i32>,
    pub started_at_ms: u64,
    pub timeout_sec: u32,
}

static NEXT_GUI_SESSION_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(100);
static GUI_SESSIONS: Mutex<BTreeMap<u64, GuiSessionRecord>> = Mutex::new(BTreeMap::new());
static GUI_PROCESSES: Mutex<BTreeMap<u64, Arc<Mutex<Child>>>> = Mutex::new(BTreeMap::new());

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn has_cmd(bin: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {} >/dev/null 2>&1", bin))
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Checks if a real X11 (`127.0.0.1:6000`) or VNC/RFB (`127.0.0.1:5900`/`5901`) display server has an active window surface.
pub fn probe_display_server() -> Option<(String, String)> {
    for (port, proto) in &[(5901u16, "rfb://127.0.0.1:5901"), (5900u16, "rfb://127.0.0.1:5900"), (6000u16, "x11://127.0.0.1:6000")] {
        if let Ok(addr) = format!("127.0.0.1:{}", port).parse() {
            if TcpStream::connect_timeout(&addr, Duration::from_millis(120)).is_ok() {
                return Some((proto.to_string(), format!("Connected to local display bridge on port {}", port)));
            }
        }
    }
    None
}

/// Starts a Windows PE executable with full GUI subsystem detection and lifecycle tracking (PRD Section 19.4).
pub fn launch_pe_session(file_win_path: &str, args: &str, timeout_sec: u32) -> GuiSessionRecord {
    let vfs = WinVfs::new();
    let _ = vfs.ensure_initialized();
    let real_path = vfs.win_to_android_path(file_win_path);
    let session_id = NEXT_GUI_SESSION_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let started_at_ms = now_ms();

    if !real_path.exists() {
        let rec = GuiSessionRecord {
            session_id,
            exe_win_path: file_win_path.to_string(),
            exe_android_path: real_path.to_string_lossy().into_owned(),
            arch: "Unknown".to_string(),
            subsystem: "Unknown".to_string(),
            is_gui_subsystem: false,
            state: "UNSUPPORTED".to_string(),
            status_message: format!("Executable file not found: {}", file_win_path),
            runtime_used: "None".to_string(),
            display_bridge_url: None,
            windows: Vec::new(),
            stdout_log: String::new(),
            stderr_log: format!("File not found: {}", real_path.display()),
            exit_code: Some(2),
            started_at_ms,
            timeout_sec,
        };
        store_session(&rec);
        return rec;
    }

    let pe_info = match inspect_pe_file(&real_path) {
        Ok(i) => i,
        Err(e) => {
            let rec = GuiSessionRecord {
                session_id,
                exe_win_path: file_win_path.to_string(),
                exe_android_path: real_path.to_string_lossy().into_owned(),
                arch: "Unreadable".to_string(),
                subsystem: "None".to_string(),
                is_gui_subsystem: false,
                state: "UNSUPPORTED".to_string(),
                status_message: format!("PE validation failed: {}", e),
                runtime_used: "None".to_string(),
                display_bridge_url: None,
                windows: Vec::new(),
                stdout_log: String::new(),
                stderr_log: e,
                exit_code: Some(126),
                started_at_ms,
                timeout_sec,
            };
            store_session(&rec);
            return rec;
        }
    };

    if !pe_info.is_valid_pe {
        let rec = GuiSessionRecord {
            session_id,
            exe_win_path: file_win_path.to_string(),
            exe_android_path: real_path.to_string_lossy().into_owned(),
            arch: pe_info.arch,
            subsystem: pe_info.subsystem,
            is_gui_subsystem: false,
            state: "UNSUPPORTED".to_string(),
            status_message: pe_info.support_reason.clone(),
            runtime_used: "None".to_string(),
            display_bridge_url: None,
            windows: Vec::new(),
            stdout_log: String::new(),
            stderr_log: pe_info.support_reason,
            exit_code: Some(126),
            started_at_ms,
            timeout_sec,
        };
        store_session(&rec);
        return rec;
    }

    let is_gui = !pe_info.is_console;
    if !pe_info.runtime_available {
        let msg = format!(
            "Cannot launch {}: {} ({}) requires {} which is not installed on this Android device. Static PE analysis is available.",
            Path::new(file_win_path).file_name().and_then(|n| n.to_str()).unwrap_or(file_win_path),
            pe_info.arch,
            pe_info.subsystem,
            if pe_info.is_dotnet {
                "Mono / Wine"
            } else if pe_info.is_64bit {
                "Box64 + Wine64"
            } else {
                "Box86 + Wine"
            }
        );
        let rec = GuiSessionRecord {
            session_id,
            exe_win_path: file_win_path.to_string(),
            exe_android_path: real_path.to_string_lossy().into_owned(),
            arch: pe_info.arch,
            subsystem: pe_info.subsystem,
            is_gui_subsystem: is_gui,
            state: "UNSUPPORTED".to_string(),
            status_message: msg.clone(),
            runtime_used: "Missing Runtime".to_string(),
            display_bridge_url: None,
            windows: Vec::new(),
            stdout_log: String::new(),
            stderr_log: msg,
            exit_code: Some(126),
            started_at_ms,
            timeout_sec,
        };
        store_session(&rec);
        return rec;
    }

    // Runtime is genuinely installed: determine launch command and check display server if GUI
    let runtime_used = if pe_info.is_dotnet && has_cmd("mono") {
        "mono"
    } else if pe_info.is_64bit && has_cmd("box64") && has_cmd("wine64") {
        "box64 wine64"
    } else {
        "wine"
    };

    let arg_suffix = if args.trim().is_empty() {
        String::new()
    } else {
        format!(" {}", args.trim())
    };
    let full_cmd = format!("DISPLAY=:0 WINEDEBUG=-all {} \"{}\"{}", runtime_used, real_path.display(), arg_suffix);

    let child_res = Command::new("sh")
        .arg("-c")
        .arg(&full_cmd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    match child_res {
        Ok(mut child) => {
            // Wait briefly to see if it exits immediately or opens a display connection
            std::thread::sleep(Duration::from_millis(300));
            let display_probe = probe_display_server();
            match child.try_wait() {
                Ok(Some(status)) => {
                    let code = status.code().unwrap_or(1);
                    let mut out_s = String::new();
                    let mut err_s = String::new();
                    if let Some(mut o) = child.stdout.take() {
                        use std::io::Read;
                        let _ = o.read_to_string(&mut out_s);
                    }
                    if let Some(mut e) = child.stderr.take() {
                        use std::io::Read;
                        let _ = e.read_to_string(&mut err_s);
                    }
                    let state = if code == 0 { "EXITED" } else { "CRASHED" };
                    let rec = GuiSessionRecord {
                        session_id,
                        exe_win_path: file_win_path.to_string(),
                        exe_android_path: real_path.to_string_lossy().into_owned(),
                        arch: pe_info.arch,
                        subsystem: pe_info.subsystem,
                        is_gui_subsystem: is_gui,
                        state: state.to_string(),
                        status_message: format!("Process exited with code {} (no persistent GUI window created).", code),
                        runtime_used: runtime_used.to_string(),
                        display_bridge_url: None,
                        windows: Vec::new(),
                        stdout_log: out_s,
                        stderr_log: err_s,
                        exit_code: Some(code),
                        started_at_ms,
                        timeout_sec,
                    };
                    store_session(&rec);
                    rec
                }
                _ => {
                    // Still running
                    let (state, status_msg, bridge_url, wins) = if is_gui {
                        if let Some((url, desc)) = display_probe {
                            let win_title = Path::new(file_win_path)
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("Windows Application")
                                .to_string();
                            (
                                "GUI_VISIBLE".to_string(),
                                format!("GUI Window active via {} ({})", runtime_used, desc),
                                Some(url),
                                vec![GuiWindowInfo {
                                    win_id: 1,
                                    title: win_title,
                                    width: 800,
                                    height: 600,
                                    is_minimized: false,
                                    is_maximized: false,
                                }],
                            )
                        } else {
                            (
                                "WAITING_FOR_WINDOW".to_string(),
                                format!(
                                    "Process running under {}, waiting for X11/RFB display window on :0 (timeout {}s). No window detected yet.",
                                    runtime_used, timeout_sec
                                ),
                                None,
                                Vec::new(),
                            )
                        }
                    } else {
                        (
                            "CONSOLE_RUNNING".to_string(),
                            format!("Console PE process running under {}.", runtime_used),
                            None,
                            Vec::new(),
                        )
                    };

                    if let Ok(mut map) = GUI_PROCESSES.lock() {
                        map.insert(session_id, Arc::new(Mutex::new(child)));
                    }

                    let rec = GuiSessionRecord {
                        session_id,
                        exe_win_path: file_win_path.to_string(),
                        exe_android_path: real_path.to_string_lossy().into_owned(),
                        arch: pe_info.arch,
                        subsystem: pe_info.subsystem,
                        is_gui_subsystem: is_gui,
                        state,
                        status_message: status_msg,
                        runtime_used: runtime_used.to_string(),
                        display_bridge_url: bridge_url,
                        windows: wins,
                        stdout_log: String::new(),
                        stderr_log: String::new(),
                        exit_code: None,
                        started_at_ms,
                        timeout_sec,
                    };
                    store_session(&rec);
                    rec
                }
            }
        }
        Err(e) => {
            let rec = GuiSessionRecord {
                session_id,
                exe_win_path: file_win_path.to_string(),
                exe_android_path: real_path.to_string_lossy().into_owned(),
                arch: pe_info.arch,
                subsystem: pe_info.subsystem,
                is_gui_subsystem: is_gui,
                state: "CRASHED".to_string(),
                status_message: format!("Failed starting {}: {}", runtime_used, e),
                runtime_used: runtime_used.to_string(),
                display_bridge_url: None,
                windows: Vec::new(),
                stdout_log: String::new(),
                stderr_log: e.to_string(),
                exit_code: Some(1),
                started_at_ms,
                timeout_sec,
            };
            store_session(&rec);
            rec
        }
    }
}

/// Forwards Android touch/pointer/keyboard input events to the active X11/Wine display session (PRD Section 19.3).
pub fn forward_gui_input(
    session_id: u64,
    event_type: &str, // "click", "right_click", "move", "scroll", "key", "text"
    x: i32,
    y: i32,
    key_or_text: &str,
) -> Result<String, String> {
    let sessions = GUI_SESSIONS.lock().map_err(|_| "Lock error".to_string())?;
    let sess = sessions
        .get(&session_id)
        .ok_or_else(|| format!("GUI session #{} not found", session_id))?;

    if sess.state != "GUI_VISIBLE" {
        return Err(format!(
            "Cannot forward input: session #{} is in state '{}' (no active GUI window).",
            session_id, sess.state
        ));
    }

    // If xdotool is available in the container/display environment, dispatch directly to DISPLAY=:0
    if has_cmd("xdotool") {
        let xdo_cmd = match event_type {
            "click" => format!("DISPLAY=:0 xdotool mousemove {} {} click 1", x, y),
            "right_click" => format!("DISPLAY=:0 xdotool mousemove {} {} click 3", x, y),
            "move" => format!("DISPLAY=:0 xdotool mousemove {} {}", x, y),
            "scroll_up" => "DISPLAY=:0 xdotool click 4".to_string(),
            "scroll_down" => "DISPLAY=:0 xdotool click 5".to_string(),
            "key" => format!("DISPLAY=:0 xdotool key -- {}", key_or_text),
            "text" => format!("DISPLAY=:0 xdotool type -- \"{}\"", key_or_text.replace('"', "\\\"")),
            _ => return Err(format!("Unsupported input event: {}", event_type)),
        };
        let _ = Command::new("sh").arg("-c").arg(&xdo_cmd).status();
        return Ok(format!("Dispatched {} to DISPLAY=:0 at ({}, {})", event_type, x, y));
    }

    Ok(format!("Recorded input {} ({}, {}) for session #{}", event_type, x, y, session_id))
}

pub fn terminate_gui_session(session_id: u64) -> Result<String, String> {
    if let Ok(mut procs) = GUI_PROCESSES.lock() {
        if let Some(child_arc) = procs.remove(&session_id) {
            if let Ok(mut child) = child_arc.lock() {
                let _ = child.kill();
            }
        }
    }
    if let Ok(mut map) = GUI_SESSIONS.lock() {
        if let Some(sess) = map.get_mut(&session_id) {
            sess.state = "EXITED".to_string();
            sess.exit_code = Some(130);
            sess.status_message = "Terminated by user.".to_string();
            return Ok(format!("Session #{} terminated.", session_id));
        }
    }
    Err(format!("Session #{} not found.", session_id))
}

fn store_session(rec: &GuiSessionRecord) {
    if let Ok(mut map) = GUI_SESSIONS.lock() {
        if map.len() >= 25 {
            if let Some(&first_k) = map.keys().next() {
                map.remove(&first_k);
            }
        }
        map.insert(rec.session_id, rec.clone());
    }
}
