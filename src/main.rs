//! Unified CLI, Windows VFS Manager, Batch Interpreter, PE Analyzer & Local HTTP/MCP Server (`wsr` v4.0).

use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process;

use wsr_jni::mcp::{extract_json_str, handle_mcp_call, mcp_tools_schema_json};
use wsr_jni::pe::{format_pe_summary, inspect_pe_file};
use wsr_jni::reg::{format_reg_report, parse_and_apply_reg, read_reg_file};
use wsr_jni::runner::{
    execute_job, expand_tilde, probe_file, replace_extension, run_environment_check,
};
use wsr_jni::server::start_http_server;
use wsr_jni::transpiler::transpile_bat;
use wsr_jni::vfs::WinVfs;

fn print_usage() {
    eprintln!("Windows Script Runner (Rust Engine v4.0.0 - Native Batch Interpreter + Honest PE Analyzer)");
    eprintln!("Usage:");
    eprintln!("  wsr vfs-init                          Initialize Windows VFS (C:\\, D:\\, Z:\\)");
    eprintln!("  wsr probe <file>                      Probe file execution support mode & launch plan");
    eprintln!("  wsr exec <script> [args...]           Execute script via built-in Batch/VFS Job Manager");
    eprintln!("  wsr pe <file.exe|file.dll>            Inspect Windows PE binary header, sections & imports");
    eprintln!("  wsr reg <file.reg> [--apply]          Preview or apply .reg file to isolated Virtual Registry");
    eprintln!("  wsr transpile <file.bat|file.cmd>     Transpile .bat/.cmd to POSIX .sh");
    eprintln!("  wsr list [dir_or_drive]               List directory entries as JSON");
    eprintln!("  wsr check-env                         Check VFS mounts and installed runtimes");
    eprintln!("  wsr http-serve [port]                 Start local HTTP bridge server (default: 18765)");
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
            println!("wsr 4.0.0 (Rust Native Batch Interpreter + Honest PE Analyzer)");
        }
        "vfs-init" => {
            println!("Windows VFS initialized:");
            println!("  {}", vfs.describe_drive_backing("C:\\"));
            println!("  {}", vfs.describe_drive_backing("D:\\"));
            println!("  {}", vfs.describe_drive_backing("Z:\\"));
        }
        "probe" => {
            if args.len() < 3 {
                eprintln!("Usage: wsr probe <file>");
                process::exit(1);
            }
            let p = probe_file(&args[2], "");
            println!("File:        {}", p.file_path);
            println!("Format:      {}", p.format_label);
            println!("Mode:        {}", p.mode.as_str());
            println!("Executable:  {}", p.can_execute);
            println!("Reason:      {}", p.reason);
            println!("Launch Plan: {}", p.launch_plan);
        }
        "exec" | "run" => {
            if args.len() < 3 {
                eprintln!("Usage: wsr exec <script> [args...]");
                process::exit(1);
            }
            let extra = if args.len() > 3 { args[3..].join(" ") } else { String::new() };
            let job = execute_job(&args[2], true, &extra, "C:\\Users\\Admin\\Desktop");
            if !job.stdout.is_empty() {
                println!("{}", job.stdout);
            }
            if !job.stderr.is_empty() {
                eprintln!("{}", job.stderr);
            }
            process::exit(job.exit_code);
        }
        "pe" => {
            if args.len() < 3 {
                eprintln!("Usage: wsr pe <file.exe|file.dll>");
                process::exit(1);
            }
            let real_p = vfs.win_to_android_path(&args[2]);
            match inspect_pe_file(&real_p) {
                Ok(info) => println!("{}", format_pe_summary(Path::new(&args[2]), &info)),
                Err(e) => {
                    eprintln!("{}", e);
                    process::exit(1);
                }
            }
        }
        "reg" => {
            if args.len() < 3 {
                eprintln!("Usage: wsr reg <file.reg> [--apply]");
                process::exit(1);
            }
            let apply = args.iter().any(|a| a == "--apply");
            let real_p = vfs.win_to_android_path(&args[2]);
            match read_reg_file(&real_p) {
                Ok(content) => {
                    let rep = parse_and_apply_reg(&content, &args[2], apply);
                    println!("{}", format_reg_report(&rep));
                }
                Err(e) => {
                    eprintln!("{}", e);
                    process::exit(1);
                }
            }
        }
        "transpile" => {
            if args.len() < 3 {
                eprintln!("Usage: wsr transpile <file.bat>");
                process::exit(1);
            }
            let file_path = expand_tilde(&args[2]);
            let src = fs::read_to_string(&file_path).unwrap_or_else(|e| {
                eprintln!("Error reading {}: {}", file_path, e);
                process::exit(1);
            });
            let res = transpile_bat(&src);
            let out_path = replace_extension(&file_path, "sh");
            fs::write(&out_path, &res.script).unwrap_or_else(|e| {
                eprintln!("Error writing {}: {}", out_path, e);
                process::exit(1);
            });
            println!("Written: {}", out_path);
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
