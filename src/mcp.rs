//! JSON API & MCP Dispatcher for Windows Script Runner (PRD v4.0).

use crate::pe::{format_pe_summary, inspect_pe_file};
use crate::reg::{format_reg_report, parse_and_apply_reg, read_reg_file};
use crate::runner::{
    cancel_active_job, execute_job, expand_tilde, list_directory_native, probe_file,
    replace_extension, run_environment_check,
};
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
    "name": "wsr_list_directory",
    "description": "List files and folders in a Windows VFS drive (C:\\, D:\\, Z:\\) with explicit execution support modes.",
    "input_schema": {
      "type": "object",
      "properties": {
        "path": { "type": "string", "description": "Windows drive path (e.g. C:\\Users\\Admin\\Desktop)" }
      },
      "required": ["path"]
    }
  },
  {
    "name": "wsr_probe_file",
    "description": "Probe a file to determine its execution mode (NATIVE, INTERPRETED, EMULATED, ANALYSIS ONLY, UNSUPPORTED), launch plan, and reason.",
    "input_schema": {
      "type": "object",
      "properties": {
        "file_path": { "type": "string" },
        "args": { "type": "string" }
      },
      "required": ["file_path"]
    }
  },
  {
    "name": "wsr_run_script",
    "description": "Execute a supported script (.bat, .cmd, .reg, .sh, or runtime-verified .ps1/.vbs/.exe) via the Job Manager.",
    "input_schema": {
      "type": "object",
      "properties": {
        "file_path": { "type": "string" },
        "args": { "type": "string" },
        "cwd": { "type": "string" }
      },
      "required": ["file_path"]
    }
  },
  {
    "name": "wsr_exec_command",
    "description": "Execute an interactive command line inside the Rust Windows Batch/CMD interpreter.",
    "input_schema": {
      "type": "object",
      "properties": {
        "cmd": { "type": "string" },
        "cwd": { "type": "string" }
      },
      "required": ["cmd"]
    }
  },
  {
    "name": "wsr_cancel_job",
    "description": "Cancel/terminate the currently running job.",
    "input_schema": { "type": "object", "properties": {} }
  },
  {
    "name": "wsr_inspect_exe",
    "description": "Inspect a Windows PE (.exe, .dll, .scr, .com, .msi) file header, sections, entry point, and imported DLLs without executing it.",
    "input_schema": {
      "type": "object",
      "properties": { "file_path": { "type": "string" } },
      "required": ["file_path"]
    }
  }
]"#
}

pub fn handle_mcp_call(tool_name: &str, raw_args_json: &str) -> String {
    let vfs = WinVfs::new();
    let _ = vfs.ensure_initialized();

    match tool_name {
        "wsr_list_directory" => {
            let path = extract_json_str(raw_args_json, "path")
                .unwrap_or_else(|| "C:\\Users\\Admin\\Desktop".to_string());
            let resolved_android = vfs.win_to_android_path(&path);
            let resolved_win = vfs.android_to_win_path(&resolved_android);
            let backing = vfs.describe_drive_backing(&resolved_win);
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
                        "{{\"status\":\"ok\",\"android_path\":\"{}\",\"win_path\":\"{}\",\"backing\":\"{}\",\"entries\":[{}]}}",
                        json_escape(&resolved_android.to_string_lossy()),
                        json_escape(&resolved_win),
                        json_escape(&backing),
                        items.join(",")
                    )
                }
                Err(err) => format!("{{\"status\":\"error\",\"error\":\"{}\"}}", json_escape(&err)),
            }
        }
        "wsr_probe_file" => {
            let file_path = extract_json_str(raw_args_json, "file_path").unwrap_or_default();
            let args = extract_json_str(raw_args_json, "args").unwrap_or_default();
            let p = probe_file(&file_path, &args);
            let real_p = vfs.win_to_android_path(&file_path);
            format!(
                "{{\"status\":\"ok\",\"file_path\":\"{}\",\"android_path\":\"{}\",\"format_label\":\"{}\",\"mode\":\"{}\",\"can_execute\":{},\"can_analyze\":{},\"reason\":\"{}\",\"launch_plan\":\"{}\"}}",
                json_escape(&p.file_path),
                json_escape(&real_p.to_string_lossy()),
                json_escape(&p.format_label),
                json_escape(p.mode.as_str()),
                p.can_execute,
                p.can_analyze,
                json_escape(&p.reason),
                json_escape(&p.launch_plan)
            )
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
                        "{{\"status\":\"ok\",\"win_path\":\"{}\",\"android_path\":\"{}\",\"pe_status\":\"{}\",\"is_valid_pe\":{},\"is_dll\":{},\"arch\":\"{}\",\"subsystem\":\"{}\",\"entry_point\":\"0x{:08x}\",\"image_base\":\"0x{:x}\",\"is_dotnet\":{},\"runtime_available\":{},\"support_level\":\"{}\",\"support_reason\":\"{}\",\"sections\":[{}],\"imported_dlls\":[{}],\"summary\":\"{}\"}}",
                        json_escape(&win_p),
                        json_escape(&real_p.to_string_lossy()),
                        json_escape(&info.status),
                        info.is_valid_pe,
                        info.is_dll,
                        json_escape(&info.arch),
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
                "{{\"status\":\"ok\",\"vfs_drive_c\":\"{}\",\"vfs_drive_d\":\"{}\",\"vfs_drive_z\":\"{}\",\"message\":\"Windows VFS drives (C:, D:, Z:) and built-in Rust Batch/PE/Registry engines initialized.\"}}",
                json_escape(&vfs.drive_c.to_string_lossy()),
                json_escape(&vfs.drive_d.to_string_lossy()),
                json_escape(&vfs.drive_z.to_string_lossy())
            )
        }
        _ => format!("{{\"status\":\"error\",\"error\":\"Unknown tool: {}\"}}", json_escape(tool_name)),
    }
}
