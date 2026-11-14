//! JSON-RPC / MCP & HTTP API Dispatcher in Pure Rust.

use crate::pe::{format_pe_summary, inspect_pe_file};
use crate::reg::{format_reg_report, parse_and_apply_reg, read_reg_file};
use crate::runner::{
    build_command, detect_strategy, execute_direct, expand_tilde, list_directory_native,
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
    "description": "List files and folders in a Windows VFS drive (C:\\, D:\\, Z:\\) or Android directory using the Rust VFS scanner.",
    "input_schema": {
      "type": "object",
      "properties": {
        "path": { "type": "string", "description": "Windows drive path (e.g. C:\\Users\\Admin) or POSIX path" }
      },
      "required": ["path"]
    }
  },
  {
    "name": "wsr_run_script",
    "description": "Execute a Windows or POSIX script/binary on Android (.bat, .cmd, .ps1, .vbs, .reg, .exe, .msi, .rs, .py, .js, .sh).",
    "input_schema": {
      "type": "object",
      "properties": {
        "file_path": { "type": "string", "description": "Full Windows or Android path to the script or .exe" },
        "args": { "type": "string", "description": "Optional arguments to pass to the script" }
      },
      "required": ["file_path"]
    }
  },
  {
    "name": "wsr_exec_command",
    "description": "Execute an interactive command or script directly inside the Rust Windows VFS environment.",
    "input_schema": {
      "type": "object",
      "properties": {
        "cmd": { "type": "string", "description": "Command string to execute" }
      },
      "required": ["cmd"]
    }
  },
  {
    "name": "wsr_inspect_exe",
    "description": "Inspect a Windows PE (.exe, .dll, .msi) header to identify CPU architecture, subsystem, and .NET CLR metadata.",
    "input_schema": {
      "type": "object",
      "properties": {
        "file_path": { "type": "string", "description": "Path to the .exe or .dll file" }
      },
      "required": ["file_path"]
    }
  },
  {
    "name": "wsr_read_script",
    "description": "Read the contents of a script file and return as UTF-8 text.",
    "input_schema": {
      "type": "object",
      "properties": {
        "file_path": { "type": "string", "description": "Full path to the script file" }
      },
      "required": ["file_path"]
    }
  },
  {
    "name": "wsr_transpile_bat",
    "description": "Transpile a .bat or .cmd file to POSIX shell (.sh) with Windows VFS path translation.",
    "input_schema": {
      "type": "object",
      "properties": {
        "file_path": { "type": "string", "description": "Path to the .bat or .cmd file" }
      },
      "required": ["file_path"]
    }
  },
  {
    "name": "wsr_setup_tools",
    "description": "Initialize the Windows VFS (C:\\, D:\\, Z:\\) and compile the native Rust helper binaries (bat2sh, reg_parser, wsr).",
    "input_schema": {
      "type": "object",
      "properties": {}
    }
  },
  {
    "name": "wsr_check_environment",
    "description": "Check Windows VFS mounts and availability of rustc, cargo, box64, wine, mono, pwsh, and helper tools.",
    "input_schema": {
      "type": "object",
      "properties": {}
    }
  }
]"#
}

pub fn handle_mcp_call(tool_name: &str, raw_args_json: &str) -> String {
    let vfs = WinVfs::new();
    let _ = vfs.ensure_initialized();

    match tool_name {
        "wsr_list_directory" => {
            let path = extract_json_str(raw_args_json, "path").unwrap_or_else(|| "C:\\Users\\Admin\\Desktop".to_string());
            let resolved_android = vfs.win_to_android_path(&path);
            let resolved_win = vfs.android_to_win_path(&resolved_android);
            match list_directory_native(&path) {
                Ok(entries) => {
                    let mut items = Vec::new();
                    for e in entries {
                        let strat = e.strategy.map(|s| s.as_str()).unwrap_or("none");
                        let runnable = e.strategy.map(|s| s.is_runnable()).unwrap_or(false);
                        let pe = e.pe_arch.unwrap_or_default();
                        items.push(format!(
                            "{{\"name\":\"{}\",\"path\":\"{}\",\"win_path\":\"{}\",\"is_dir\":{},\"size\":{},\"strategy\":\"{}\",\"runnable\":{},\"pe_arch\":\"{}\"}}",
                            json_escape(&e.name),
                            json_escape(&e.path),
                            json_escape(&e.win_path),
                            e.is_dir,
                            e.size,
                            strat,
                            runnable,
                            json_escape(&pe)
                        ));
                    }
                    format!(
                        "{{\"status\":\"ok\",\"android_path\":\"{}\",\"win_path\":\"{}\",\"entries\":[{}]}}",
                        json_escape(&resolved_android.to_string_lossy()),
                        json_escape(&resolved_win),
                        items.join(",")
                    )
                }
                Err(err) => format!("{{\"status\":\"error\",\"error\":\"{}\"}}", json_escape(&err)),
            }
        }
        "wsr_run_script" => {
            let file_path = extract_json_str(raw_args_json, "file_path").unwrap_or_default();
            let args = extract_json_str(raw_args_json, "args").unwrap_or_default();
            let direct = extract_json_str(raw_args_json, "direct").unwrap_or_default() == "true";
            match detect_strategy(&file_path) {
                Some(strat) => {
                    let cmd = build_command(&file_path, strat, &args);
                    if direct {
                        let (output, exit_code) = execute_direct(&file_path, true, &args);
                        format!(
                            "{{\"status\":\"ok\",\"strategy\":\"{}\",\"command\":\"{}\",\"output\":\"{}\",\"exit\":{}}}",
                            strat.as_str(),
                            json_escape(&cmd),
                            json_escape(&output),
                            exit_code
                        )
                    } else {
                        format!(
                            "{{\"status\":\"ok\",\"strategy\":\"{}\",\"command\":\"{}\"}}",
                            strat.as_str(),
                            json_escape(&cmd)
                        )
                    }
                }
                None => format!(
                    "{{\"status\":\"error\",\"error\":\"Unsupported script extension: {}\"}}",
                    json_escape(&file_path)
                ),
            }
        }
        "wsr_exec_command" => {
            let cmd = extract_json_str(raw_args_json, "cmd").unwrap_or_default();
            let (output, exit_code) = execute_direct(&cmd, false, "");
            format!(
                "{{\"status\":\"ok\",\"output\":\"{}\",\"exit\":{}}}",
                json_escape(&output),
                exit_code
            )
        }
        "wsr_inspect_exe" => {
            let file_path = expand_tilde(&extract_json_str(raw_args_json, "file_path").unwrap_or_default());
            let p = Path::new(&file_path);
            match inspect_pe_file(p) {
                Ok(info) => {
                    let summary = format_pe_summary(p, &info);
                    format!(
                        "{{\"status\":\"ok\",\"is_valid_pe\":{},\"arch\":\"{}\",\"subsystem\":\"{}\",\"is_dotnet\":{},\"runner\":\"{}\",\"summary\":\"{}\"}}",
                        info.is_valid_pe,
                        json_escape(&info.arch),
                        json_escape(&info.subsystem),
                        info.is_dotnet,
                        json_escape(&info.recommended_runner),
                        json_escape(&summary)
                    )
                }
                Err(e) => format!("{{\"status\":\"error\",\"error\":\"{}\"}}", json_escape(&e)),
            }
        }
        "wsr_read_script" => {
            let file_path = expand_tilde(&extract_json_str(raw_args_json, "file_path").unwrap_or_default());
            match fs::read_to_string(&file_path) {
                Ok(content) => format!("{{\"status\":\"ok\",\"content\":\"{}\"}}", json_escape(&content)),
                Err(e) => format!("{{\"status\":\"error\",\"error\":\"{}\"}}", json_escape(&e.to_string())),
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
        "wsr_parse_reg" => {
            let file_path = expand_tilde(&extract_json_str(raw_args_json, "file_path").unwrap_or_default());
            match read_reg_file(Path::new(&file_path)) {
                Ok(src) => {
                    let rep = parse_and_apply_reg(&src, &file_path, true);
                    let formatted = format_reg_report(&rep);
                    format!("{{\"status\":\"ok\",\"report\":\"{}\"}}", json_escape(&formatted))
                }
                Err(e) => format!("{{\"status\":\"error\",\"error\":\"{}\"}}", json_escape(&e)),
            }
        }
        "wsr_check_environment" => {
            let rep = run_environment_check();
            format!("{{\"status\":\"ok\",\"report\":\"{}\"}}", json_escape(&rep))
        }
        "wsr_setup_tools" => {
            let _ = vfs.ensure_initialized();
            let cmd = "mkdir -p ~/.antigem_tools/bin && rustc ~/.antigem_tools/src/bat2sh.rs -O -o ~/.antigem_tools/bin/bat2sh && rustc ~/.antigem_tools/src/reg_parser.rs -O -o ~/.antigem_tools/bin/reg_parser && chmod +x ~/.antigem_tools/bin/*";
            format!(
                "{{\"status\":\"ok\",\"vfs_drive_c\":\"{}\",\"command\":\"{}\"}}",
                json_escape(&vfs.drive_c.to_string_lossy()),
                json_escape(cmd)
            )
        }
        _ => format!("{{\"status\":\"error\",\"error\":\"Unknown MCP tool: {}\"}}", json_escape(tool_name)),
    }
}
