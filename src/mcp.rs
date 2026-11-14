//! JSON API & MCP Dispatcher for Windows Script Runner (v6.0 — Runtime Diagnostics, Installer & Live X11 Frame Bridge).

use crate::gui_session::{
    capture_x11_frame_base64, forward_gui_input, launch_pe_session, terminate_gui_session,
};
use crate::pe::{format_pe_summary, inspect_pe_file};
use crate::reg::{format_reg_report, parse_and_apply_reg, read_reg_file};
use crate::runner::{
    cancel_active_job, execute_job, expand_tilde, list_directory_native, probe_file,
    replace_extension, run_environment_check,
};
use crate::runtime_mgr::{probe_runtime_environment, run_runtime_installer};
use crate::transpiler::transpile_bat;
use crate::vfs::WinVfs;
use std::fs;
use std::path::Path;

pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

pub fn extract_json_str(json: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{}\"", key);
    let idx = json.find(&pattern)?;
    let after_key = &json[idx + pattern.len()..];
    let colon = after_key.find(':')?;
    let after_colon = after_key[colon + 1..].trim_start();
    if !after_colon.starts_with('"') {
        return None;
    }
    let mut out = String::new();
    let mut chars = after_colon[1..].chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(esc) = chars.next() {
                match esc {
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    other => out.push(other),
                }
            }
        } else if c == '"' {
            return Some(out);
        } else {
            out.push(c);
        }
    }
    None
}

pub fn mcp_tools_schema_json() -> &'static str {
    r#"[
  {
    "name": "wsr_runtime_diagnostics",
    "description": "Return full runtime diagnostics: installed components (Box64, Wine64 x64, Wine ARM64, Xvfb, xdotool, Mono), supported PE architectures (x86, x64, ARM64, ARM64EC), and technical blockers.",
    "input_schema": { "type": "object", "properties": {} }
  },
  {
    "name": "wsr_install_runtime",
    "description": "Generate and execute the Box64 + Wine64 + Xvfb runtime installer script and validate installed components.",
    "input_schema": { "type": "object", "properties": {} }
  },
  {
    "name": "wsr_list_directory",
    "description": "List files and folders in a Windows VFS drive or authorized Android storage folder.",
    "input_schema": {
      "type": "object",
      "properties": { "path": { "type": "string" } },
      "required": ["path"]
    }
  },
  {
    "name": "wsr_inspect_exe",
    "description": "Inspect a Windows PE (.exe, .dll, .scr, .com, .msi) file and separately classify x86, x64, ARM64, and ARM64EC.",
    "input_schema": {
      "type": "object",
      "properties": { "file_path": { "type": "string" } },
      "required": ["file_path"]
    }
  },
  {
    "name": "wsr_launch_pe_gui",
    "description": "Validate and launch a Windows PE executable under the matching runtime with X11 window detection and live frame capture.",
    "input_schema": {
      "type": "object",
      "properties": {
        "file_path": { "type": "string" },
        "args": { "type": "string" },
        "timeout_sec": { "type": "string" }
      },
      "required": ["file_path"]
    }
  }
]"#
}

pub fn handle_mcp_call(tool_name: &str, raw_args_json: &str) -> String {
    let vfs = WinVfs::new();
    let _ = vfs.ensure_initialized();

    match tool_name {
        "wsr_runtime_diagnostics" => {
            let d = probe_runtime_environment();
            let comps_json: Vec<String> = d
                .components
                .iter()
                .map(|c| {
                    format!(
                        "{{\"id\":\"{}\",\"name\":\"{}\",\"installed\":{},\"version\":\"{}\",\"role\":\"{}\"}}",
                        json_escape(&c.id),
                        json_escape(&c.name),
                        c.installed,
                        json_escape(&c.version_or_path),
                        json_escape(&c.role)
                    )
                })
                .collect();
            let arch_json: Vec<String> = d
                .arch_matrix
                .iter()
                .map(|a| {
                    format!(
                        "{{\"arch_code\":\"{}\",\"machine_hex\":\"{}\",\"required_stack\":\"{}\",\"supported_now\":{},\"status_label\":\"{}\",\"notes\":\"{}\"}}",
                        json_escape(&a.arch_code),
                        json_escape(&a.machine_hex),
                        json_escape(&a.required_stack),
                        a.supported_now,
                        json_escape(&a.status_label),
                        json_escape(&a.notes)
                    )
                })
                .collect();
            let cmds_json: Vec<String> = d
                .install_commands
                .iter()
                .map(|s| format!("\"{}\"", json_escape(s)))
                .collect();
            format!(
                "{{\"status\":\"ok\",\"host_os\":\"{}\",\"host_arch\":\"{}\",\"in_apk_sandbox\":{},\"display_server_active\":{},\"display_endpoint\":\"{}\",\"can_run_x64_pe\":{},\"can_run_x86_pe\":{},\"can_run_arm64_pe\":{},\"can_run_arm64ec_pe\":{},\"blocker_summary\":\"{}\",\"components\":[{}],\"arch_matrix\":[{}],\"install_commands\":[{}]}}",
                json_escape(&d.host_os),
                json_escape(&d.host_arch),
                d.in_apk_sandbox,
                d.display_server_active,
                json_escape(&d.display_endpoint),
                d.can_run_x64_pe,
                d.can_run_x86_pe,
                d.can_run_arm64_pe,
                d.can_run_arm64ec_pe,
                json_escape(&d.blocker_summary),
                comps_json.join(","),
                arch_json.join(","),
                cmds_json.join(",")
            )
        }
        "wsr_install_runtime" => {
            let (ok, log) = run_runtime_installer();
            format!(
                "{{\"status\":\"ok\",\"installed_ready\":{},\"log\":\"{}\"}}",
                ok,
                json_escape(&log)
            )
        }
        "wsr_gui_frame" => {
            let sid = extract_json_str(raw_args_json, "session_id")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            match capture_x11_frame_base64(sid) {
                Ok(b64) => format!("{{\"status\":\"ok\",\"frame_base64\":\"{}\"}}", b64),
                Err(e) => format!("{{\"status\":\"error\",\"error\":\"{}\"}}", json_escape(&e)),
            }
        }
        "wsr_list_directory" => {
            let path = extract_json_str(raw_args_json, "path")
                .unwrap_or_else(|| "C:\\Users\\Admin\\Desktop".to_string());
            let loc_status = vfs.inspect_storage_source(&path);
            let resolved_android = vfs.win_to_android_path(&path);
            let resolved_win = vfs.android_to_win_path(&resolved_android);

            match list_directory_native(&path) {
                Ok(entries) => {
                    let mut items = Vec::new();
                    for e in entries {
                        items.push(format!(
                            "{{\"name\":\"{}\",\"path\":\"{}\",\"win_path\":\"{}\",\"is_dir\":{},\"size\":{},\"format_label\":\"{}\",\"mode\":\"{}\",\"can_execute\":{},\"can_analyze\":{}}}",
                            json_escape(&e.name),
                            json_escape(&e.path),
                            json_escape(&e.win_path),
                            e.is_dir,
                            e.size,
                            json_escape(&e.format_label),
                            json_escape(&e.mode),
                            e.can_execute,
                            e.can_analyze
                        ));
                    }
                    format!(
                        "{{\"status\":\"ok\",\"android_path\":\"{}\",\"win_path\":\"{}\",\"slot\":\"{}\",\"is_real_android\":{},\"is_authorized\":{},\"backing\":\"{}\",\"entries\":[{}]}}",
                        json_escape(&resolved_android.to_string_lossy()),
                        json_escape(&resolved_win),
                        json_escape(&loc_status.slot),
                        loc_status.is_real_android,
                        loc_status.is_authorized,
                        json_escape(&loc_status.backing_desc),
                        items.join(",")
                    )
                }
                Err(err) => format!("{{\"status\":\"error\",\"error\":\"{}\"}}", json_escape(&err)),
            }
        }
        "wsr_save_android_mount" => {
            let slot = extract_json_str(raw_args_json, "slot").unwrap_or_else(|| "downloads".to_string());
            let uri = extract_json_str(raw_args_json, "uri").unwrap_or_default();
            match vfs.save_android_mount(&slot, &uri) {
                Ok(()) => format!(
                    "{{\"status\":\"ok\",\"slot\":\"{}\",\"uri\":\"{}\"}}",
                    json_escape(&slot),
                    json_escape(&uri)
                ),
                Err(e) => format!("{{\"status\":\"error\",\"error\":\"{}\"}}", json_escape(&e)),
            }
        }
        "wsr_probe_file" => {
            let file_path = extract_json_str(raw_args_json, "file_path").unwrap_or_default();
            let args = extract_json_str(raw_args_json, "args").unwrap_or_default();
            let p = probe_file(&file_path, &args);
            let real_p = vfs.win_to_android_path(&file_path);
            let is_gui_pe = inspect_pe_file(&real_p)
                .ok()
                .map(|i| i.is_valid_pe && !i.is_console && !i.is_dll)
                .unwrap_or(false);
            format!(
                "{{\"status\":\"ok\",\"file_path\":\"{}\",\"android_path\":\"{}\",\"format_label\":\"{}\",\"mode\":\"{}\",\"can_execute\":{},\"can_analyze\":{},\"is_gui_pe\":{},\"reason\":\"{}\",\"launch_plan\":\"{}\"}}",
                json_escape(&p.file_path),
                json_escape(&real_p.to_string_lossy()),
                json_escape(&p.format_label),
                json_escape(p.mode.as_str()),
                p.can_execute,
                p.can_analyze,
                is_gui_pe,
                json_escape(&p.reason),
                json_escape(&p.launch_plan)
            )
        }
        "wsr_launch_pe_gui" => {
            let file_path = extract_json_str(raw_args_json, "file_path").unwrap_or_default();
            let args = extract_json_str(raw_args_json, "args").unwrap_or_default();
            let timeout_sec = extract_json_str(raw_args_json, "timeout_sec")
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(8);
            let rec = launch_pe_session(&file_path, &args, timeout_sec);
            let wins_json: Vec<String> = rec
                .windows
                .iter()
                .map(|w| {
                    format!(
                        "{{\"win_id\":\"{}\",\"title\":\"{}\",\"width\":{},\"height\":{},\"is_minimized\":{},\"is_maximized\":{}}}",
                        json_escape(&w.win_id),
                        json_escape(&w.title),
                        w.width,
                        w.height,
                        w.is_minimized,
                        w.is_maximized
                    )
                })
                .collect();
            format!(
                "{{\"status\":\"ok\",\"session_id\":{},\"exe_win_path\":\"{}\",\"arch_enum\":\"{}\",\"arch\":\"{}\",\"subsystem\":\"{}\",\"is_gui_subsystem\":{},\"state\":\"{}\",\"status_message\":\"{}\",\"runtime_used\":\"{}\",\"display_bridge_url\":\"{}\",\"windows\":[{}],\"stdout\":\"{}\",\"stderr\":\"{}\",\"exit_code\":{}}}",
                rec.session_id,
                json_escape(&rec.exe_win_path),
                json_escape(&rec.arch_enum),
                json_escape(&rec.arch),
                json_escape(&rec.subsystem),
                rec.is_gui_subsystem,
                json_escape(&rec.state),
                json_escape(&rec.status_message),
                json_escape(&rec.runtime_used),
                json_escape(&rec.display_bridge_url.unwrap_or_default()),
                wins_json.join(","),
                json_escape(&rec.stdout_log),
                json_escape(&rec.stderr_log),
                rec.exit_code.unwrap_or(-1)
            )
        }
        "wsr_gui_input" => {
            let sid = extract_json_str(raw_args_json, "session_id")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let ev = extract_json_str(raw_args_json, "event_type").unwrap_or_default();
            let x = extract_json_str(raw_args_json, "x")
                .and_then(|s| s.parse::<i32>().ok())
                .unwrap_or(0);
            let y = extract_json_str(raw_args_json, "y")
                .and_then(|s| s.parse::<i32>().ok())
                .unwrap_or(0);
            let key = extract_json_str(raw_args_json, "key_or_text").unwrap_or_default();
            match forward_gui_input(sid, &ev, x, y, &key) {
                Ok(msg) => format!("{{\"status\":\"ok\",\"message\":\"{}\"}}", json_escape(&msg)),
                Err(e) => format!("{{\"status\":\"error\",\"error\":\"{}\"}}", json_escape(&e)),
            }
        }
        "wsr_terminate_gui" => {
            let sid = extract_json_str(raw_args_json, "session_id")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            match terminate_gui_session(sid) {
                Ok(msg) => format!("{{\"status\":\"ok\",\"message\":\"{}\"}}", json_escape(&msg)),
                Err(e) => format!("{{\"status\":\"error\",\"error\":\"{}\"}}", json_escape(&e)),
            }
        }
        "wsr_run_script" => {
            let file_path = extract_json_str(raw_args_json, "file_path").unwrap_or_default();
            let args = extract_json_str(raw_args_json, "args").unwrap_or_default();
            let cwd = extract_json_str(raw_args_json, "cwd")
                .unwrap_or_else(|| "C:\\Users\\Admin\\Desktop".to_string());
            let job = execute_job(&file_path, true, &args, &cwd);
            format!(
                "{{\"status\":\"ok\",\"job_id\":{},\"state\":\"{}\",\"mode\":\"{}\",\"command\":\"{}\",\"stdout\":\"{}\",\"stderr\":\"{}\",\"exit\":{},\"cwd_win\":\"{}\",\"title\":\"{}\"}}",
                job.job_id,
                json_escape(&job.state),
                json_escape(&job.mode),
                json_escape(&job.command),
                json_escape(&job.stdout),
                json_escape(&job.stderr),
                job.exit_code,
                json_escape(&job.cwd_win),
                json_escape(&job.title.unwrap_or_default())
            )
        }
        "wsr_exec_command" => {
            let cmd = extract_json_str(raw_args_json, "cmd").unwrap_or_default();
            let cwd = extract_json_str(raw_args_json, "cwd")
                .unwrap_or_else(|| "C:\\Users\\Admin\\Desktop".to_string());
            let job = execute_job(&cmd, false, "", &cwd);
            format!(
                "{{\"status\":\"ok\",\"job_id\":{},\"state\":\"{}\",\"mode\":\"{}\",\"stdout\":\"{}\",\"stderr\":\"{}\",\"exit\":{},\"cwd_win\":\"{}\",\"title\":\"{}\"}}",
                job.job_id,
                json_escape(&job.state),
                json_escape(&job.mode),
                json_escape(&job.stdout),
                json_escape(&job.stderr),
                job.exit_code,
                json_escape(&job.cwd_win),
                json_escape(&job.title.unwrap_or_default())
            )
        }
        "wsr_cancel_job" => {
            if let Some(jid) = cancel_active_job() {
                format!("{{\"status\":\"ok\",\"cancelled_job_id\":{}}}", jid)
            } else {
                "{\"status\":\"ok\",\"cancelled_job_id\":0}".to_string()
            }
        }
        "wsr_inspect_exe" => {
            let raw_fp = extract_json_str(raw_args_json, "file_path").unwrap_or_default();
            let real_p = vfs.win_to_android_path(&raw_fp);
            let win_p = vfs.android_to_win_path(&real_p);
            match inspect_pe_file(&real_p) {
                Ok(info) => {
                    let summary = format_pe_summary(Path::new(&win_p), &info);
                    let secs_json: Vec<String> = info
                        .sections
                        .iter()
                        .map(|s| format!("\"{}\"", json_escape(s)))
                        .collect();
                    let dlls_json: Vec<String> = info
                        .imported_dlls
                        .iter()
                        .map(|d| format!("\"{}\"", json_escape(d)))
                        .collect();
                    format!(
                        "{{\"status\":\"ok\",\"win_path\":\"{}\",\"android_path\":\"{}\",\"pe_status\":\"{}\",\"is_valid_pe\":{},\"is_dll\":{},\"is_console\":{},\"arch_enum\":\"{}\",\"arch\":\"{}\",\"required_backend\":\"{}\",\"subsystem\":\"{}\",\"entry_point\":\"0x{:08x}\",\"image_base\":\"0x{:x}\",\"is_dotnet\":{},\"runtime_available\":{},\"support_level\":\"{}\",\"support_reason\":\"{}\",\"sections\":[{}],\"imported_dlls\":[{}],\"summary\":\"{}\"}}",
                        json_escape(&win_p),
                        json_escape(&real_p.to_string_lossy()),
                        json_escape(&info.status),
                        info.is_valid_pe,
                        info.is_dll,
                        info.is_console,
                        json_escape(&info.arch_enum),
                        json_escape(&info.arch),
                        json_escape(&info.required_backend),
                        json_escape(&info.subsystem),
                        info.entry_point_rva,
                        info.image_base,
                        info.is_dotnet,
                        info.runtime_available,
                        json_escape(&info.support_level),
                        json_escape(&info.support_reason),
                        secs_json.join(","),
                        dlls_json.join(","),
                        json_escape(&summary)
                    )
                }
                Err(e) => format!("{{\"status\":\"error\",\"error\":\"{}\"}}", json_escape(&e)),
            }
        }
        "wsr_read_script" => {
            let raw_fp = extract_json_str(raw_args_json, "file_path").unwrap_or_default();
            let real_p = vfs.win_to_android_path(&raw_fp);
            match fs::read_to_string(&real_p) {
                Ok(content) => format!(
                    "{{\"status\":\"ok\",\"win_path\":\"{}\",\"android_path\":\"{}\",\"content\":\"{}\"}}",
                    json_escape(&vfs.android_to_win_path(&real_p)),
                    json_escape(&real_p.to_string_lossy()),
                    json_escape(&content)
                ),
                Err(e) => format!("{{\"status\":\"error\",\"error\":\"{}\"}}", json_escape(&e.to_string())),
            }
        }
        "wsr_write_file" => {
            let raw_fp = extract_json_str(raw_args_json, "file_path").unwrap_or_default();
            let content = extract_json_str(raw_args_json, "content").unwrap_or_default();
            match vfs.write_text_file(&raw_fp, &content) {
                Ok(win_p) => format!("{{\"status\":\"ok\",\"win_path\":\"{}\"}}", json_escape(&win_p)),
                Err(e) => format!("{{\"status\":\"error\",\"error\":\"{}\"}}", json_escape(&e)),
            }
        }
        "wsr_stage_file" => {
            let name = extract_json_str(raw_args_json, "name").unwrap_or_else(|| "staged.bat".to_string());
            let content = extract_json_str(raw_args_json, "content").unwrap_or_default();
            match vfs.stage_external_file(&name, content.as_bytes()) {
                Ok(win_p) => format!("{{\"status\":\"ok\",\"staged_win_path\":\"{}\"}}", json_escape(&win_p)),
                Err(e) => format!("{{\"status\":\"error\",\"error\":\"{}\"}}", json_escape(&e)),
            }
        }
        "wsr_mkdir" => {
            let raw_fp = extract_json_str(raw_args_json, "path").unwrap_or_default();
            match vfs.create_dir(&raw_fp) {
                Ok(win_p) => format!("{{\"status\":\"ok\",\"win_path\":\"{}\"}}", json_escape(&win_p)),
                Err(e) => format!("{{\"status\":\"error\",\"error\":\"{}\"}}", json_escape(&e)),
            }
        }
        "wsr_rename" => {
            let src = extract_json_str(raw_args_json, "src").unwrap_or_default();
            let dst = extract_json_str(raw_args_json, "dst").unwrap_or_default();
            match vfs.rename_item(&src, &dst) {
                Ok(win_p) => format!("{{\"status\":\"ok\",\"win_path\":\"{}\"}}", json_escape(&win_p)),
                Err(e) => format!("{{\"status\":\"error\",\"error\":\"{}\"}}", json_escape(&e)),
            }
        }
        "wsr_copy" => {
            let src = extract_json_str(raw_args_json, "src").unwrap_or_default();
            let dst = extract_json_str(raw_args_json, "dst").unwrap_or_default();
            match vfs.copy_item(&src, &dst) {
                Ok(win_p) => format!("{{\"status\":\"ok\",\"win_path\":\"{}\"}}", json_escape(&win_p)),
                Err(e) => format!("{{\"status\":\"error\",\"error\":\"{}\"}}", json_escape(&e)),
            }
        }
        "wsr_delete" => {
            let raw_fp = extract_json_str(raw_args_json, "path").unwrap_or_default();
            match vfs.delete_item(&raw_fp) {
                Ok(()) => "{\"status\":\"ok\"}".to_string(),
                Err(e) => format!("{{\"status\":\"error\",\"error\":\"{}\"}}", json_escape(&e)),
            }
        }
        "wsr_preview_reg" | "wsr_parse_reg" => {
            let raw_fp = extract_json_str(raw_args_json, "file_path").unwrap_or_default();
            let apply = extract_json_str(raw_args_json, "apply").unwrap_or_default() == "true"
                || tool_name == "wsr_parse_reg";
            let real_p = vfs.win_to_android_path(&raw_fp);
            let win_p = vfs.android_to_win_path(&real_p);
            match read_reg_file(&real_p) {
                Ok(src) => {
                    let rep = parse_and_apply_reg(&src, &win_p, apply);
                    let formatted = format_reg_report(&rep);
                    format!(
                        "{{\"status\":\"ok\",\"applied\":{},\"entries_count\":{},\"env_count\":{},\"report\":\"{}\"}}",
                        rep.applied_mode,
                        rep.entries.len(),
                        rep.env_vars.len(),
                        json_escape(&formatted)
                    )
                }
                Err(e) => format!("{{\"status\":\"error\",\"error\":\"{}\"}}", json_escape(&e)),
            }
        }
        "wsr_transpile_bat" => {
            let file_path = expand_tilde(&extract_json_str(raw_args_json, "file_path").unwrap_or_default());
            match fs::read_to_string(&file_path) {
                Ok(src) => {
                    let res = transpile_bat(&src);
                    let sh_path = replace_extension(&file_path, "sh");
                    let _ = fs::write(&sh_path, &res.script);
                    let skipped_json: Vec<String> = res
                        .skipped
                        .iter()
                        .map(|s| format!("\"{}\"", json_escape(s)))
                        .collect();
                    format!(
                        "{{\"status\":\"ok\",\"output_path\":\"{}\",\"script\":\"{}\",\"skipped\":[{}]}}",
                        json_escape(&sh_path),
                        json_escape(&res.script),
                        skipped_json.join(",")
                    )
                }
                Err(e) => format!("{{\"status\":\"error\",\"error\":\"{}\"}}", json_escape(&e.to_string())),
            }
        }
        "wsr_check_environment" => {
            let rep = run_environment_check();
            format!("{{\"status\":\"ok\",\"report\":\"{}\"}}", json_escape(&rep))
        }
        "wsr_setup_tools" => {
            let _ = vfs.ensure_initialized();
            format!(
                "{{\"status\":\"ok\",\"vfs_drive_c\":\"{}\",\"vfs_drive_d\":\"{}\",\"vfs_drive_z\":\"{}\",\"message\":\"Windows VFS drives (C:, D:, Z:), SAF Staging Workspace, and Rust Batch/PE/Registry/Runtime engines initialized.\"}}",
                json_escape(&vfs.drive_c.to_string_lossy()),
                json_escape(&vfs.drive_d.to_string_lossy()),
                json_escape(&vfs.drive_z.to_string_lossy())
            )
        }
        _ => format!("{{\"status\":\"error\",\"error\":\"Unknown tool: {}\"}}", json_escape(tool_name)),
    }
}
