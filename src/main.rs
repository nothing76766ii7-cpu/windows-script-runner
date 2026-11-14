//! Unified CLI, Windows VFS Manager, PE Inspector & Local HTTP/MCP Server (`wsr`) in Pure Rust.

use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process;

use wsr_jni::mcp::{extract_json_str, handle_mcp_call, mcp_tools_schema_json};
use wsr_jni::pe::{format_pe_summary, inspect_pe_file};
use wsr_jni::reg::{format_reg_report, parse_and_apply_reg, read_reg_file};
use wsr_jni::runner::{
    build_command, detect_strategy, execute_direct, expand_tilde, replace_extension,
    run_environment_check,
};
use wsr_jni::server::start_http_server;
use wsr_jni::transpiler::transpile_bat;
use wsr_jni::vfs::WinVfs;

fn print_usage() {
    eprintln!("Windows Script Runner (Rust Engine v3.0.0 - Windows VFS + PE EXE Runner)");
    eprintln!("Usage:");
    eprintln!("  wsr vfs-init                          Initialize Windows VFS (C:\\, D:\\, Z:\\)");
    eprintln!("  wsr vfs-path <WinPath>                Translate Windows path (C:\\...) to Android path");
    eprintln!("  wsr transpile <file.bat|file.cmd>     Transpile .bat/.cmd to POSIX .sh with VFS support");
    eprintln!("  wsr reg <file.reg>                    Parse .reg file and apply env vars to ~/.bashrc");
    eprintln!("  wsr pe <file.exe|file.dll>            Inspect Windows PE binary headers & architecture");
    eprintln!("  wsr run <script_or_exe> [args...]     Execute or print runner command for script/.exe");
    eprintln!("  wsr exec <script_or_exe> [args...]    Execute script/.exe directly inside Rust VFS");
    eprintln!("  wsr list [dir_or_drive]               List directory entries as JSON (supports C:\\, D:\\, Z:\\)");
    eprintln!("  wsr check-env                         Check VFS mounts and installed runtimes");
    eprintln!("  wsr http-serve [port]                 Start local HTTP bridge server (default: 18765)");
    eprintln!("  wsr mcp-tools                         Output MCP tools JSON schema");
    eprintln!("  wsr mcp-serve                         Start JSON-RPC MCP stdio server");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    let vfs = WinVfs::new();
    let _ = vfs.ensure_initialized();

    match args[1].as_str() {
        "--version" | "-v" => {
            println!("wsr 3.0.0 (Rust Windows VFS + PE EXE Runner)");
        }
        "vfs-init" => match vfs.ensure_initialized() {
            Ok(_) => {
                println!("Windows VFS initialized:");
                println!("  Drive C: -> {}", vfs.drive_c.display());
                println!("  Drive D: -> {}", vfs.drive_d.display());
                println!("  Drive Z: -> {}", vfs.drive_z.display());
            }
            Err(e) => {
                eprintln!("VFS initialization error: {}", e);
                process::exit(1);
            }
        },
        "vfs-path" => {
            if args.len() < 3 {
                eprintln!("Usage: wsr vfs-path <WindowsPath>");
                process::exit(1);
            }
            let p = vfs.win_to_android_path(&args[2]);
            println!("{}", p.display());
        }
        "transpile" => {
            if args.len() < 3 {
                eprintln!("Error: missing path to .bat or .cmd file");
                process::exit(1);
            }
            let file_path = expand_tilde(&args[2]);
            let src = match fs::read_to_string(&file_path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Error reading {}: {}", file_path, e);
                    process::exit(1);
                }
            };
            let res = transpile_bat(&src);
            let out_path = replace_extension(&file_path, "sh");
            if let Err(e) = fs::write(&out_path, &res.script) {
                eprintln!("Error writing {}: {}", out_path, e);
                process::exit(1);
            }
            println!("Written: {}", out_path);
            if !res.skipped.is_empty() {
                println!("\nSkipped {} Windows-only lines:", res.skipped.len());
                for s in &res.skipped {
                    println!("  # [SKIP Win32: {}]", s);
                }
            }
        }
        "reg" => {
            if args.len() < 3 {
                eprintln!("Error: missing path to .reg file");
                process::exit(1);
            }
            let file_path = expand_tilde(&args[2]);
            match read_reg_file(Path::new(&file_path)) {
                Ok(content) => {
                    let report = parse_and_apply_reg(&content, &file_path, true);
                    println!("{}", format_reg_report(&report));
                }
                Err(e) => {
                    eprintln!("{}", e);
                    process::exit(1);
                }
            }
        }
        "pe" => {
            if args.len() < 3 {
                eprintln!("Error: missing path to .exe or .dll file");
                process::exit(1);
            }
            let file_path = expand_tilde(&args[2]);
            let p = Path::new(&file_path);
            match inspect_pe_file(p) {
                Ok(info) => println!("{}", format_pe_summary(p, &info)),
                Err(e) => {
                    eprintln!("{}", e);
                    process::exit(1);
                }
            }
        }
        "run" => {
            if args.len() < 3 {
                eprintln!("Error: missing script or .exe path");
                process::exit(1);
            }
            let file_path = expand_tilde(&args[2]);
            let extra_args = if args.len() > 3 { args[3..].join(" ") } else { String::new() };
            match detect_strategy(&file_path) {
                Some(strat) => {
                    let cmd = build_command(&file_path, strat, &extra_args);
                    println!("{}", cmd);
                }
                None => {
                    eprintln!("Unsupported file type: {}", file_path);
                    process::exit(1);
                }
            }
        }
        "exec" => {
            if args.len() < 3 {
                eprintln!("Error: missing script or .exe path");
                process::exit(1);
            }
            let extra_args = if args.len() > 3 { args[3..].join(" ") } else { String::new() };
            let (out, code) = execute_direct(&args[2], true, &extra_args);
            print!("{}", out);
            process::exit(code);
        }
        "list" => {
            let dir = if args.len() >= 3 { &args[2] } else { "C:\\Users\\Admin\\Desktop" };
            let json_arg = format!("{{\"path\":\"{}\"}}", dir.replace('\\', "\\\\").replace('"', "\\\""));
            println!("{}", handle_mcp_call("wsr_list_directory", &json_arg));
        }
        "check-env" => {
            println!("{}", run_environment_check());
        }
        "http-serve" => {
            let port = args.get(2).and_then(|s| s.parse::<u16>().ok()).unwrap_or(18765);
            println!("Starting Windows Script Runner HTTP server on 127.0.0.1:{}...", port);
            if let Err(e) = start_http_server(port) {
                eprintln!("{}", e);
                process::exit(1);
            }
        }
        "mcp-tools" => {
            println!("{}", mcp_tools_schema_json());
        }
        "mcp-serve" => {
            let stdin = io::stdin();
            let mut stdout = io::stdout();
            for line in stdin.lock().lines().flatten() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let req_type = extract_json_str(trimmed, "type").unwrap_or_default();
                let req_id = extract_json_str(trimmed, "id").unwrap_or_else(|| "0".to_string());
                if req_type == "MCP_TOOLS_LIST" {
                    let _ = writeln!(stdout, "{{\"id\":\"{}\",\"tools\":{}}}", req_id, mcp_tools_schema_json());
                } else if req_type == "MCP_TOOL_CALL" {
                    let tool = extract_json_str(trimmed, "tool").unwrap_or_default();
                    let res = handle_mcp_call(&tool, trimmed);
                    let _ = writeln!(stdout, "{{\"id\":\"{}\",\"result\":{}}}", req_id, res);
                } else {
                    let _ = writeln!(stdout, "{{\"id\":\"{}\",\"error\":\"Unsupported request type\"}}", req_id);
                }
                let _ = stdout.flush();
            }
        }
        _ => {
            print_usage();
            process::exit(1);
        }
    }
}
