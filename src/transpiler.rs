//! Zero-dependency Rust `.bat` / `.cmd` to POSIX `.sh` transpiler with Windows VFS (`C:\`, `D:\`, `Z:\`) path translation.

use crate::vfs::WinVfs;

#[derive(Debug, Clone)]
pub struct TranspileResult {
    pub script: String,
    pub skipped: Vec<String>,
}

const WIN32_CMDS: &[&str] = &[
    "reg", "net", "sc", "netstat", "regedit",
    "rundll32", "regsvr32", "wmic", "powercfg", "bcdedit", "diskpart",
    "icacls", "attrib", "cacls", "takeown", "cipher", "compact", "defrag",
    "chkdsk", "sfc", "dism",
];

pub fn is_win32_cmd(cmd: &str) -> bool {
    let clean = cmd.trim().to_ascii_lowercase();
    let clean = clean.strip_suffix(".exe").unwrap_or(&clean);
    WIN32_CMDS.contains(&clean)
}

/// Translates embedded Windows drive paths (`C:\...`, `D:\...`, `Z:\...`) and `%VAR%` inside any argument string.
pub fn translate_win_tokens(input: &str) -> String {
    let mut s = replace_env_vars(input);
    // Translate C:\ or C:/ to ${WSR_DRIVE_C}/
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

pub fn replace_env_vars(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 16);
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '%' {
            // Handle %~dp0, %~f0, %~nx0
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

pub fn normalize_slashes(s: &str) -> String {
    translate_win_tokens(s)
}

pub fn transpile_line(line: &str, skipped: &mut Vec<String>, open_func: &mut bool) -> String {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Some(rest) = trimmed.strip_prefix("::") {
        return format!("# {}", rest.trim());
    }

    let clean = trimmed.strip_prefix('@').unwrap_or(trimmed).trim();
    let lower = clean.to_ascii_lowercase();

    // Standalone drive switch: C: or D: or Z:
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
        let target = translate_win_tokens(clean[6..].trim());
        return format!("pushd {}", target);
    }
    if lower.starts_with("title ") {
        let title_str = translate_win_tokens(clean[6..].trim()).replace('"', "\\\"");
        return format!("printf '\\033]0;%s\\007' \"{}\"", title_str);
    }
    if lower.starts_with("color ") {
        return format!("# [INFO: color {} ignored in POSIX shell]", clean[6..].trim());
    }
    if lower.starts_with("timeout ") {
        let tokens: Vec<&str> = clean.split_whitespace().collect();
        for tok in &tokens[1..] {
            if let Ok(sec) = tok.parse::<u64>() {
                return format!("sleep {}", sec);
            }
            if let Some(num) = tok.strip_prefix("/t").or_else(|| tok.strip_prefix("/T")) {
                if !num.is_empty() {
                    if let Ok(sec) = num.parse::<u64>() {
                        return format!("sleep {}", sec);
                    }
                }
            }
        }
        return "sleep 1".to_string();
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
            let val = expr[eq_idx + 1..].trim().trim_matches('"');
            let val_conv = translate_win_tokens(val);
            if is_arith {
                return format!("export {}=$(( {} ))", var, val_conv);
            } else if is_prompt {
                return format!("printf '%s' \"{}\"; read -r {}; export {}", val_conv, var, var);
            } else {
                return format!("export {}=\"{}\"", var, val_conv);
            }
        }
    }
    if lower.starts_with("goto ") {
        let label = clean[5..].trim().trim_start_matches(':').to_ascii_lowercase();
        if label == "eof" {
            return "exit 0".to_string();
        }
        return label;
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
        let rest = clean[13..].trim();
        if let Some((path_part, cmd_part)) = split_first_token(rest) {
            let mut dummy = false;
            let inner = transpile_line(cmd_part, skipped, &mut dummy);
            return format!(
                "if [ ! -e \"{}\" ]; then {}; fi",
                translate_win_tokens(path_part.trim_matches('"')),
                inner
            );
        }
    }
    if lower.starts_with("if exist ") {
        let rest = clean[9..].trim();
        if let Some((path_part, cmd_part)) = split_first_token(rest) {
            let mut dummy = false;
            let inner = transpile_line(cmd_part, skipped, &mut dummy);
            return format!(
                "if [ -e \"{}\" ]; then {}; fi",
                translate_win_tokens(path_part.trim_matches('"')),
                inner
            );
        }
    }
    if lower.starts_with("if errorlevel ") {
        let rest = clean[14..].trim();
        if let Some((lvl_part, cmd_part)) = split_first_token(rest) {
            let mut dummy = false;
            let inner = transpile_line(cmd_part, skipped, &mut dummy);
            return format!("if [ $? -ge {} ]; then {}; fi", lvl_part, inner);
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
                let mut dummy = false;
                let body_trans = transpile_line(do_body, skipped, &mut dummy);
                return format!(
                    "for {} in {}; do\n  {}\ndone",
                    var_name,
                    translate_win_tokens(&items),
                    body_trans
                );
            }
        }
    }
    if lower.starts_with("call ") {
        let target = translate_win_tokens(clean[5..].trim());
        let target = if target.ends_with(".bat") || target.ends_with(".cmd") {
            format!("{}.sh", &target[..target.len() - 4])
        } else {
            target
        };
        return format!("sh {}", target);
    }
    if lower.starts_with("start ") {
        let mut rest = clean[6..].trim();
        if rest.starts_with('"') {
            if let Some((_, after_title)) = split_first_token(rest) {
                rest = after_title;
            }
        }
        if rest.len() >= 2 && rest[..2].eq_ignore_ascii_case("/b") {
            rest = rest[2..].trim();
        }
        return format!("{} &", translate_win_tokens(rest));
    }
    if lower == "exit" || lower.starts_with("exit ") {
        let rest = clean[4..].trim();
        let rest = if rest.len() >= 2 && rest[..2].eq_ignore_ascii_case("/b") {
            rest[2..].trim()
        } else {
            rest
        };
        let code = if rest.is_empty() { "0" } else { rest };
        return format!("exit {}", code);
    }
    if lower.starts_with("mkdir ") || lower.starts_with("md ") {
        let idx = if lower.starts_with("mkdir ") { 6 } else { 3 };
        let path_arg = translate_win_tokens(clean[idx..].trim());
        return format!("mkdir -p {}", path_arg);
    }
    if lower.starts_with("rmdir ") || lower.starts_with("rd ") {
        let idx = if lower.starts_with("rmdir ") { 6 } else { 3 };
        let mut arg = clean[idx..].trim();
        loop {
            let l_arg = arg.to_ascii_lowercase();
            if l_arg.starts_with("/s") || l_arg.starts_with("/q") {
                arg = arg[2..].trim();
            } else {
                break;
            }
        }
        return format!("rm -rf {}", translate_win_tokens(arg));
    }
    if lower.starts_with("copy ") || lower.starts_with("xcopy ") || lower.starts_with("robocopy ") {
        let idx = if lower.starts_with("robocopy ") {
            9
        } else if lower.starts_with("xcopy ") {
            6
        } else {
            5
        };
        let mut rest = clean[idx..].trim();
        while rest.starts_with('/') {
            if let Some((_, next)) = split_first_token(rest) {
                rest = next;
            } else {
                break;
            }
        }
        let conv = translate_win_tokens(rest);
        return format!("cp -r {}", conv);
    }
    if lower.starts_with("move ") {
        let mut rest = clean[5..].trim();
        if rest.len() >= 2 && rest[..2].eq_ignore_ascii_case("/y") {
            rest = rest[2..].trim();
        }
        let conv = translate_win_tokens(rest);
        return format!("mv -f {}", conv);
    }
    if lower.starts_with("ren ") || lower.starts_with("rename ") {
        let idx = if lower.starts_with("rename ") { 7 } else { 4 };
        let conv = translate_win_tokens(clean[idx..].trim());
        return format!("mv -f {}", conv);
    }
    if lower.starts_with("del ") || lower.starts_with("erase ") {
        let idx = if lower.starts_with("erase ") { 6 } else { 4 };
        let mut rest = clean[idx..].trim();
        loop {
            let l_rest = rest.to_ascii_lowercase();
            if l_rest.starts_with("/f") || l_rest.starts_with("/q") || l_rest.starts_with("/s") {
                rest = rest[2..].trim();
            } else {
                break;
            }
        }
        let conv = translate_win_tokens(rest);
        return format!("rm -f {}", conv);
    }
    if lower.starts_with("type ") {
        let conv = translate_win_tokens(clean[5..].trim());
        return format!("cat {}", conv);
    }
    if lower == "cd" || lower.starts_with("cd ") || lower.starts_with("chdir ") {
        let idx = if lower.starts_with("chdir ") { 6 } else if lower.len() > 2 { 3 } else { 2 };
        let mut rest = clean[idx..].trim();
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

pub fn transpile_bat(src: &str) -> TranspileResult {
    let vfs = WinVfs::new();
    let _ = vfs.ensure_initialized();
    let mut out = vec![
        "#!/bin/sh".to_string(),
        "# Generated by Windows Script Runner (Rust bat2sh + Windows VFS Engine)".to_string(),
        "set -e".to_string(),
        vfs.posix_env_preamble(),
    ];
    let mut skipped = Vec::new();
    let mut open_func = false;

    for raw_line in src.lines() {
        out.push(transpile_line(raw_line, &mut skipped, &mut open_func));
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

    TranspileResult {
        script: out.join("\n") + "\n",
        skipped,
    }
}
