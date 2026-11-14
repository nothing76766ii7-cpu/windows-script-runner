#!/usr/bin/env rust-script
//! Standalone zero-dependency Rust `.bat` / `.cmd` to POSIX `.sh` transpiler with Windows VFS (`C:\`, `D:\`, `Z:\`).
//! Compile directly with: `rustc tools/bat2sh.rs -O -o ~/.antigem_tools/bin/bat2sh`

use std::env;
use std::fs;
use std::path::Path;
use std::process;

const WIN32_CMDS: &[&str] = &[
    "reg", "net", "sc", "netstat", "regedit",
    "rundll32", "regsvr32", "wmic", "powercfg", "bcdedit", "diskpart",
    "icacls", "attrib", "cacls", "takeown", "cipher", "compact", "defrag",
    "chkdsk", "sfc", "dism",
];

fn is_win32_cmd(cmd: &str) -> bool {
    let clean = cmd.trim().to_ascii_lowercase();
    let clean = clean.strip_suffix(".exe").unwrap_or(&clean);
    WIN32_CMDS.contains(&clean)
}

fn translate_win_tokens(input: &str) -> String {
    let mut s = replace_env_vars(input);
    for (drive_pat, env_var) in &[
        ("C:\\", "${WSR_DRIVE_C}/"),
        ("c:\\", "${WSR_DRIVE_C}/"),
        ("C:/", "${WSR_DRIVE_C}/"),
        ("c:/", "${WSR_DRIVE_C}/"),
        ("D:\\", "${WSR_DRIVE_D}/"),
        ("d:\\", "${WSR_DRIVE_D}/"),
        ("D:/", "${WSR_DRIVE_D}/"),
        ("d:/", "${WSR_DRIVE_D}/"),
        ("Z:\\", "/"),
        ("z:\\", "/"),
        ("Z:/", "/"),
        ("z:/", "/"),
    ] {
        s = s.replace(drive_pat, env_var);
    }
    s.replace('\\', "/")
}

fn replace_env_vars(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 16);
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '%' {
            if i + 2 < chars.len() && chars[i + 1] == '~' {
                let mut j = i + 2;
                while j < chars.len() && chars[j].is_ascii_alphabetic() {
                    j += 1;
                }
                if j < chars.len() && chars[j].is_ascii_digit() {
                    let mods: String = chars[i + 2..j].iter().collect();
                    let digit = chars[j];
                    if mods.eq_ignore_ascii_case("dp") && digit == '0' {
                        out.push_str("$(cd \"$(dirname \"$0\")\" && pwd)/");
                    } else {
                        out.push('$');
                        out.push(digit);
                    }
                    i = j + 1;
                    continue;
                }
            }
            if i + 1 < chars.len() && chars[i + 1] == '%' {
                if i + 2 < chars.len() && chars[i + 2].is_ascii_alphanumeric() {
                    out.push('$');
                    out.push(chars[i + 2].to_ascii_lowercase());
                    i += 3;
                    continue;
                }
            }
            if i + 1 < chars.len() && chars[i + 1] == '*' {
                out.push_str("\"$@\"");
                i += 2;
                continue;
            }
            if i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
                out.push('$');
                out.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if let Some(rel_end) = chars[i + 1..].iter().position(|&c| c == '%') {
                let end = i + 1 + rel_end;
                let var_name: String = chars[i + 1..end].iter().collect();
                if !var_name.is_empty()
                    && var_name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '(' || c == ')')
                {
                    let mapped = match var_name.to_ascii_uppercase().as_str() {
                        "CD" => "PWD".to_string(),
                        "RANDOM" => "RANDOM".to_string(),
                        "ERRORLEVEL" => "?".to_string(),
                        "USERPROFILE" | "HOMEPATH" => "USERPROFILE".to_string(),
                        "SYSTEMDRIVE" => "SystemDrive".to_string(),
                        "SYSTEMROOT" | "WINDIR" => "SystemRoot".to_string(),
                        "PROGRAMFILES" | "PROGRAMFILES(X86)" => "ProgramFiles".to_string(),
                        "PROGRAMDATA" | "ALLUSERSPROFILE" => "ProgramData".to_string(),
                        "APPDATA" => "APPDATA".to_string(),
                        "LOCALAPPDATA" => "LOCALAPPDATA".to_string(),
                        "TEMP" | "TMP" => "TEMP".to_string(),
                        _ => var_name,
                    };
                    if mapped == "?" {
                        out.push_str("$?");
                    } else {
                        out.push_str(&format!("${{{}}}", mapped));
                    }
                    i = end + 1;
                    continue;
                }
            }
            out.push('$');
            i += 1;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn split_first_token(s: &str) -> Option<(&str, &str)> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(stripped) = s.strip_prefix('"') {
        if let Some(end_quote) = stripped.find('"') {
            let token = &s[..end_quote + 2];
            let rest = s[end_quote + 2..].trim();
            return Some((token, rest));
        }
    }
    let mut iter = s.splitn(2, char::is_whitespace);
    let first = iter.next()?;
    let rest = iter.next().unwrap_or("").trim();
    Some((first, rest))
}

fn transpile_line(line: &str, skipped: &mut Vec<String>, open_func: &mut bool) -> String {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Some(rest) = trimmed.strip_prefix("::") {
        return format!("# {}", rest.trim());
    }

    let clean = trimmed.strip_prefix('@').unwrap_or(trimmed).trim();
    let lower = clean.to_ascii_lowercase();

    if clean.len() == 2 && clean.as_bytes()[1] == b':' && clean.as_bytes()[0].is_ascii_alphabetic() {
        let drv = (clean.as_bytes()[0] as char).to_ascii_uppercase();
        return match drv {
            'C' => "cd \"${WSR_DRIVE_C}\"".to_string(),
            'D' => "cd \"${WSR_DRIVE_D}\"".to_string(),
            'Z' => "cd /".to_string(),
            _ => "cd \"${WSR_DRIVE_C}\"".to_string(),
        };
    }

    if lower == "setlocal" || lower.starts_with("setlocal ") || lower == "endlocal" || lower.starts_with("endlocal ") {
        return format!("# [VFS: {}]", clean);
    }
    if lower == "echo off" || lower.starts_with("echo off ") {
        return "set +x".to_string();
    }
    if lower == "echo on" || lower.starts_with("echo on ") {
        return "set -x".to_string();
    }
    if lower == "echo." || lower == "echo:" {
        return "echo \"\"".to_string();
    }
    if lower.starts_with("echo ") {
        let msg = &clean[5..];
        if let Some(redir_idx) = msg.find('>') {
            let (text_part, redir_part) = msg.split_at(redir_idx);
            let is_append = redir_part.starts_with(">>");
            let target_raw = if is_append { &redir_part[2..] } else { &redir_part[1..] }.trim();
            let target_conv = translate_win_tokens(target_raw.trim_matches('"'));
            let text_conv = translate_win_tokens(text_part.trim()).replace('"', "\\\"");
            let op = if is_append { ">>" } else { ">" };
            return format!("echo \"{}\" {} \"{}\"", text_conv, op, target_conv);
        }
        let converted = translate_win_tokens(msg).replace('"', "\\\"");
        return format!("echo \"{}\"", converted);
    }
    if lower == "rem" || lower.starts_with("rem ") {
        let comment = if clean.len() > 3 { clean[3..].trim() } else { "" };
        return format!("# {}", comment);
    }
    if lower == "cls" {
        return "clear".to_string();
    }
    if lower == "ver" {
        return "echo \"Microsoft Windows [Version 10.0.22631.4317 - Android Rust VFS]\"".to_string();
    }
    if lower == "dir" || lower.starts_with("dir ") {
        let rest = if clean.len() > 3 { clean[3..].trim() } else { "" };
        let arg = rest
            .split_whitespace()
            .filter(|t| !t.starts_with('/'))
            .collect::<Vec<_>>()
            .join(" ");
        let target = translate_win_tokens(&arg);
        return if target.is_empty() {
            "ls -la".to_string()
        } else {
            format!("ls -la {}", target)
        };
    }
    if lower == "ipconfig" || lower.starts_with("ipconfig ") {
        return "ip addr show 2>/dev/null || ifconfig 2>/dev/null || echo 'Active Network Interface: wlan0 (Android VFS)'".to_string();
    }
    if lower == "tasklist" || lower.starts_with("tasklist ") {
        return "ps -ef 2>/dev/null || ps".to_string();
    }
    if lower == "systeminfo" || lower.starts_with("systeminfo ") {
        return "echo \"OS Name: Microsoft Windows 11 Pro (Android Rust VFS)\" && uname -a".to_string();
    }
    if lower.starts_with("where ") {
        return format!("which {}", translate_win_tokens(clean[6..].trim()));
    }
    if lower.starts_with("findstr ") {
        let rest = clean[8..].trim();
        let mut flags = String::from("-E");
        let mut tokens = Vec::new();
        for tok in rest.split_whitespace() {
            if tok.eq_ignore_ascii_case("/i") {
                flags.push_str(" -i");
            } else if tok.eq_ignore_ascii_case("/n") {
                flags.push_str(" -n");
            } else if !tok.starts_with('/') {
                tokens.push(translate_win_tokens(tok));
            }
        }
        return format!("grep {} {}", flags, tokens.join(" "));
    }
    if lower == "pause" || lower.starts_with("pause ") {
        return "printf 'Press enter to continue...'; read -r _p".to_string();
    }
    if lower == "popd" {
        return "popd".to_string();
    }
    if lower.starts_with("pushd ") {
        return format!("pushd {}", translate_win_tokens(clean[6..].trim()));
    }
    if lower.starts_with("set ") {
        let rest = clean[4..].trim();
        let (is_arith, is_prompt, expr) = if rest.len() >= 2 && rest[..2].eq_ignore_ascii_case("/a") {
            (true, false, rest[2..].trim())
        } else if rest.len() >= 2 && rest[..2].eq_ignore_ascii_case("/p") {
            (false, true, rest[2..].trim())
        } else {
            (false, false, rest)
        };
        if let Some(eq_idx) = expr.find('=') {
            let var = expr[..eq_idx].trim();
            let val = translate_win_tokens(expr[eq_idx + 1..].trim().trim_matches('"'));
            if is_arith {
                return format!("export {}=$(( {} ))", var, val);
            } else if is_prompt {
                return format!("printf '%s' \"{}\"; read -r {}; export {}", val, var, var);
            } else {
                return format!("export {}=\"{}\"", var, val);
            }
        }
    }
    if lower.starts_with("goto ") {
        let label = clean[5..].trim().trim_start_matches(':').to_ascii_lowercase();
        return if label == "eof" { "exit 0".to_string() } else { label };
    }
    if clean.starts_with(':') {
        let label_name: String = clean[1..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if !label_name.is_empty() {
            let prefix = if *open_func { "}\n\n" } else { "\n" };
            *open_func = true;
            return format!("{}{}() {{", prefix, label_name.to_ascii_lowercase());
        }
    }
    if lower.starts_with("if not exist ") {
        if let Some((p, cmd)) = split_first_token(clean[13..].trim()) {
            let mut d = false;
            return format!(
                "if [ ! -e \"{}\" ]; then {}; fi",
                translate_win_tokens(p.trim_matches('"')),
                transpile_line(cmd, skipped, &mut d)
            );
        }
    }
    if lower.starts_with("if exist ") {
        if let Some((p, cmd)) = split_first_token(clean[9..].trim()) {
            let mut d = false;
            return format!(
                "if [ -e \"{}\" ]; then {}; fi",
                translate_win_tokens(p.trim_matches('"')),
                transpile_line(cmd, skipped, &mut d)
            );
        }
    }
    if lower.starts_with("if errorlevel ") {
        if let Some((lvl, cmd)) = split_first_token(clean[14..].trim()) {
            let mut d = false;
            return format!("if [ $? -ge {} ]; then {}; fi", lvl, transpile_line(cmd, skipped, &mut d));
        }
    }
    if lower.starts_with("for ") {
        if let Some(in_pos) = lower.find(" in (") {
            let var_part = clean[4..in_pos].trim().trim_start_matches('%');
            let var_name = var_part.chars().next().unwrap_or('i').to_ascii_lowercase();
            let after_in = &clean[in_pos + 5..];
            if let Some(close_paren) = after_in.find(')') {
                let items = after_in[..close_paren].replace(',', " ");
                let after_paren = after_in[close_paren + 1..].trim();
                let do_body = if after_paren.len() >= 3 && after_paren[..3].eq_ignore_ascii_case("do ") {
                    after_paren[3..].trim()
                } else {
                    after_paren
                };
                let mut d = false;
                return format!(
                    "for {} in {}; do\n  {}\ndone",
                    var_name,
                    translate_win_tokens(&items),
                    transpile_line(do_body, skipped, &mut d)
                );
            }
        }
    }
    if lower.starts_with("call ") {
        let t = translate_win_tokens(clean[5..].trim());
        let t = if t.ends_with(".bat") || t.ends_with(".cmd") {
            format!("{}.sh", &t[..t.len() - 4])
        } else {
            t
        };
        return format!("sh {}", t);
    }
    if lower == "exit" || lower.starts_with("exit ") {
        let rest = clean[4..].trim();
        let rest = if rest.len() >= 2 && rest[..2].eq_ignore_ascii_case("/b") {
            rest[2..].trim()
        } else {
            rest
        };
        return format!("exit {}", if rest.is_empty() { "0" } else { rest });
    }
    if lower.starts_with("mkdir ") || lower.starts_with("md ") {
        let idx = if lower.starts_with("mkdir ") { 6 } else { 3 };
        return format!("mkdir -p {}", translate_win_tokens(clean[idx..].trim()));
    }
    if lower.starts_with("rmdir ") || lower.starts_with("rd ") {
        let idx = if lower.starts_with("rmdir ") { 6 } else { 3 };
        let mut arg = clean[idx..].trim();
        while arg.len() >= 2 && (arg[..2].eq_ignore_ascii_case("/s") || arg[..2].eq_ignore_ascii_case("/q")) {
            arg = arg[2..].trim();
        }
        return format!("rm -rf {}", translate_win_tokens(arg));
    }
    if lower.starts_with("copy ") || lower.starts_with("xcopy ") {
        let idx = if lower.starts_with("xcopy ") { 6 } else { 5 };
        return format!("cp -r {}", translate_win_tokens(clean[idx..].trim()));
    }
    if lower.starts_with("move ") {
        return format!("mv -f {}", translate_win_tokens(clean[5..].trim()));
    }
    if lower.starts_with("del ") || lower.starts_with("erase ") {
        let idx = if lower.starts_with("erase ") { 6 } else { 4 };
        return format!("rm -f {}", translate_win_tokens(clean[idx..].trim()));
    }
    if lower.starts_with("type ") {
        return format!("cat {}", translate_win_tokens(clean[5..].trim()));
    }
    if lower == "cd" || lower.starts_with("cd ") {
        let mut rest = if clean.len() > 2 { clean[3..].trim() } else { "" };
        if rest.len() >= 2 && rest[..2].eq_ignore_ascii_case("/d") {
            rest = rest[2..].trim();
        }
        let conv = translate_win_tokens(rest);
        return if conv.is_empty() {
            "cd \"${USERPROFILE}\"".to_string()
        } else {
            format!("cd {}", conv)
        };
    }

    let first_word = clean.split_whitespace().next().unwrap_or("");
    if first_word.to_ascii_lowercase().ends_with(".exe") && !is_win32_cmd(first_word) {
        let exe_cmd = translate_win_tokens(clean);
        return format!(
            "if command -v box64 >/dev/null 2>&1 && command -v wine64 >/dev/null 2>&1; then box64 wine64 {}; elif command -v wine >/dev/null 2>&1; then wine {}; else {}; fi",
            exe_cmd, exe_cmd, exe_cmd
        );
    }

    if is_win32_cmd(first_word) {
        skipped.push(trimmed.to_string());
        return format!("# [SKIP Win32: {}]", trimmed);
    }

    translate_win_tokens(clean)
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 || args[1] == "--version" {
        println!("bat2sh 3.0.0 (Rust Windows VFS + POSIX Shell Transpiler)");
        if args.len() < 2 {
            eprintln!("Usage: bat2sh <path/to/script.bat>");
            process::exit(1);
        }
        return;
    }

    let input_path = Path::new(&args[1]);
    let src = match fs::read_to_string(input_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to read {}: {}", input_path.display(), e);
            process::exit(1);
        }
    };

    let home = env::var("HOME").unwrap_or_else(|_| "/data/data/com.termux/files/home".to_string());
    let drive_c = format!("{}/.wsr_vfs/drive_c", home);
    let _ = fs::create_dir_all(format!("{}/Windows/System32", drive_c));
    let _ = fs::create_dir_all(format!("{}/Program Files", drive_c));
    let _ = fs::create_dir_all(format!("{}/Users/Admin/Desktop", drive_c));
    let _ = fs::create_dir_all(format!("{}/Users/Admin/AppData/Local/Temp", drive_c));

    let preamble = format!(
        "export WSR_DRIVE_C=\"{c}\"\n\
         export WSR_DRIVE_D=\"/sdcard\"\n\
         export WSR_DRIVE_Z=\"/\"\n\
         export SystemDrive=\"C:\"\n\
         export SystemRoot=\"{c}/Windows\"\n\
         export windir=\"{c}/Windows\"\n\
         export ProgramFiles=\"{c}/Program Files\"\n\
         export USERPROFILE=\"{c}/Users/Admin\"\n\
         export TEMP=\"{c}/Users/Admin/AppData/Local/Temp\"\n\
         export TMP=\"{c}/Users/Admin/AppData/Local/Temp\"\n",
        c = drive_c
    );

    let mut out = vec![
        "#!/bin/sh".to_string(),
        "# Transpiled by Rust bat2sh v3.0 (Windows VFS enabled)".to_string(),
        "set -e".to_string(),
        preamble,
    ];
    let mut skipped = Vec::new();
    let mut open_func = false;

    for raw in src.lines() {
        out.push(transpile_line(raw, &mut skipped, &mut open_func));
    }
    if open_func {
        out.push("}".to_string());
    }
    if !skipped.is_empty() {
        out.push(String::new());
        out.push("# === SKIPPED Windows-only lines ===".to_string());
        for s in &skipped {
            out.push(format!("# {}", s));
        }
    }

    let out_path = input_path.with_extension("sh");
    let final_script = out.join("\n") + "\n";
    if let Err(e) = fs::write(&out_path, &final_script) {
        eprintln!("Failed to write {}: {}", out_path.display(), e);
        process::exit(1);
    }

    println!("Written: {}", out_path.display());
    if !skipped.is_empty() {
        println!("\nSkipped {} Windows-only lines:", skipped.len());
        for s in &skipped {
            println!("  # [SKIP Win32: {}]", s);
        }
    }
}
