#!/usr/bin/env rust-script
//! Standalone zero-dependency Rust Windows `.reg` parser for AntiGem.
//! Compile directly with: `rustc tools/reg_parser.rs -O -o ~/.antigem_tools/bin/reg_parser`

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;

fn read_reg_text(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        let u16_words: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        Ok(String::from_utf16_lossy(&u16_words))
    } else {
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 || args[1] == "--version" {
        println!("reg_parser 2.0.0 (Rust Registry Parser)");
        if args.len() < 2 {
            eprintln!("Usage: reg_parser <path/to/file.reg>");
            process::exit(1);
        }
        return;
    }

    let reg_path = Path::new(&args[1]);
    let content = match read_reg_text(reg_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}", e);
            process::exit(1);
        }
    };

    let home_dir = env::var("HOME").unwrap_or_else(|_| "/data/data/com.termux/files/home".to_string());
    let bashrc_path = PathBuf::from(&home_dir).join(".bashrc");
    let mut existing_bashrc = if bashrc_path.exists() {
        fs::read_to_string(&bashrc_path).unwrap_or_default()
    } else {
        String::new()
    };

    let mut current_key: Option<String> = None;
    let mut applied: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();

    for raw in content.lines() {
        let line = raw.trim().trim_start_matches('\u{feff}');
        if line.is_empty()
            || line.starts_with(';')
            || line.starts_with("Windows Registry Editor")
            || line.starts_with("REGEDIT4")
        {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            current_key = Some(line[1..line.len() - 1].to_string());
            continue;
        }
        if let Some(ref key) = current_key {
            if let Some(eq_idx) = line.find('=') {
                let n = line[..eq_idx].trim().trim_matches('"');
                let v = line[eq_idx + 1..].trim();
                let key_low = key.to_ascii_lowercase();
                let is_env = key_low.contains("hkcu\\environment")
                    || key_low.contains("hkey_current_user\\environment")
                    || key_low.contains("currentcontrolset\\control\\session manager\\environment");
                let v_low = v.to_ascii_lowercase();
                let is_bin = v_low.starts_with("hex:")
                    || v_low.starts_with("dword:")
                    || v_low.starts_with("qword:")
                    || v_low.starts_with("hex(");

                if is_env && !is_bin && n != "@" {
                    let val = v
                        .trim_matches('"')
                        .replace("\\\\", "/")
                        .replace("%SystemRoot%", "/system")
                        .replace("%USERPROFILE%", &home_dir)
                        .replace("%TEMP%", "/tmp")
                        .replace("%TMP%", "/tmp");
                    let bash_line = format!("export {}=\"{}\"", n, val);
                    if !existing_bashrc.contains(&bash_line) {
                        if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&bashrc_path) {
                            let _ = writeln!(f, "\n{}", bash_line);
                            existing_bashrc.push('\n');
                            existing_bashrc.push_str(&bash_line);
                        }
                    }
                    applied.push(format!("  {} = {}", n, val));
                } else {
                    skipped.push(format!("  [{}] {} (Windows-only or binary)", key, n));
                }
            }
        }
    }

    println!("=== .reg Parse Report ===");
    println!("File: {}", reg_path.display());
    println!("\nApplied to ~/.bashrc ({}):", applied.len());
    if applied.is_empty() {
        println!("  (none)");
    } else {
        for a in &applied {
            println!("{}", a);
        }
    }
    println!("\nSkipped - Windows-only ({}):", skipped.len());
    if skipped.is_empty() {
        println!("  (none)");
    } else {
        for s in skipped.iter().take(10) {
            println!("{}", s);
        }
        if skipped.len() > 10 {
            println!("  ...and {} more", skipped.len() - 10);
        }
    }
    println!("\nRun: source ~/.bashrc  to reload env vars");
}
