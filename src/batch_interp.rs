//! Built-in Native Rust Windows Batch / CMD Interpreter (PRD Section 6).
//! Directly parses and executes `.bat` and `.cmd` scripts and interactive DOS commands inside the
//! isolated Windows VFS (`C:\`, `D:\`, `Z:\`), displaying canonical Windows paths (`C:\Users\Admin\...`),
//! supporting `set /a` arithmetic, `setlocal enabledelayedexpansion` (`!VAR!`), `if`/`else`, `for %%v in (...)`,
//! `goto :label`, `call`, `shift`, `&&`, `||`, `&`, redirection (`>`, `>>`), and line-specific diagnostics
//! for unsupported syntax without leaking raw ANSI/OSC escape sequences.

use crate::reg::load_vfs_env_vars;
use crate::vfs::WinVfs;
use std::collections::BTreeMap;
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct BatchExecOutcome {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub final_cwd_win: String,
    pub title: Option<String>,
    pub cancelled: bool,
}

pub struct BatchSession {
    pub vfs: WinVfs,
    pub env_map: BTreeMap<String, String>,
    pub env_stack: Vec<(BTreeMap<String, String>, bool)>,
    pub delayed_expansion: bool,
    pub echo_on: bool,
    pub cwd_win: String,
    pub args: Vec<String>,
    pub script_name: String,
    pub last_exit_code: i32,
    pub title: Option<String>,
    pub stdout_lines: Vec<String>,
    pub stderr_lines: Vec<String>,
    pub cancel_flag: Option<Arc<AtomicBool>>,
}

impl BatchSession {
    pub fn new(vfs: WinVfs, initial_cwd_win: &str, script_name: &str, args: Vec<String>) -> Self {
        let mut env_map = BTreeMap::new();
        env_map.insert("SystemDrive".to_string(), "C:".to_string());
        env_map.insert("SystemRoot".to_string(), "C:\\Windows".to_string());
        env_map.insert("windir".to_string(), "C:\\Windows".to_string());
        env_map.insert("ProgramFiles".to_string(), "C:\\Program Files".to_string());
        env_map.insert("ProgramFiles(x86)".to_string(), "C:\\Program Files (x86)".to_string());
        env_map.insert("ProgramData".to_string(), "C:\\ProgramData".to_string());
        env_map.insert("ALLUSERSPROFILE".to_string(), "C:\\ProgramData".to_string());
        env_map.insert("USERPROFILE".to_string(), "C:\\Users\\Admin".to_string());
        env_map.insert("HOMEPATH".to_string(), "\\Users\\Admin".to_string());
        env_map.insert("APPDATA".to_string(), "C:\\Users\\Admin\\AppData\\Roaming".to_string());
        env_map.insert("LOCALAPPDATA".to_string(), "C:\\Users\\Admin\\AppData\\Local".to_string());
        env_map.insert("TEMP".to_string(), "C:\\Users\\Admin\\AppData\\Local\\Temp".to_string());
        env_map.insert("TMP".to_string(), "C:\\Users\\Admin\\AppData\\Local\\Temp".to_string());
        env_map.insert("PUBLIC".to_string(), "C:\\Users\\Public".to_string());
        env_map.insert("OS".to_string(), "Windows_NT".to_string());
        env_map.insert("PATH".to_string(), "C:\\Windows\\System32;C:\\Windows;C:\\Program Files\\Windows Script Runner".to_string());

        // Merge user environment variables applied from .reg files in the isolated VFS registry
        for (k, v) in load_vfs_env_vars(&vfs) {
            env_map.insert(k, v);
        }

        let norm_cwd = if initial_cwd_win.trim().is_empty() {
            "C:\\Users\\Admin\\Desktop".to_string()
        } else {
            let p = vfs.win_to_android_path(initial_cwd_win);
            vfs.android_to_win_path(&p)
        };

        Self {
            vfs,
            env_map,
            env_stack: Vec::new(),
            delayed_expansion: false,
            echo_on: true,
            cwd_win: norm_cwd,
            args,
            script_name: script_name.to_string(),
            last_exit_code: 0,
            title: None,
            stdout_lines: Vec::new(),
            stderr_lines: Vec::new(),
            cancel_flag: None,
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancel_flag
            .as_ref()
            .map(|f| f.load(Ordering::SeqCst))
            .unwrap_or(false)
    }

    fn get_var_ci(&self, key: &str) -> Option<String> {
        let upper = key.to_ascii_uppercase();
        if upper == "CD" {
            return Some(self.cwd_win.clone());
        }
        if upper == "ERRORLEVEL" {
            return Some(self.last_exit_code.to_string());
        }
        if upper == "RANDOM" {
            return Some("18427".to_string());
        }
        for (k, v) in &self.env_map {
            if k.eq_ignore_ascii_case(key) {
                return Some(v.clone());
            }
        }
        None
    }

    fn set_var_ci(&mut self, key: &str, val: String) {
        let existing_key = self
            .env_map
            .keys()
            .find(|k| k.eq_ignore_ascii_case(key))
            .cloned()
            .unwrap_or_else(|| key.to_string());
        if val.is_empty() {
            self.env_map.remove(&existing_key);
        } else {
            self.env_map.insert(existing_key, val);
        }
    }

    pub fn expand_variables(&self, input: &str, loop_vars: &BTreeMap<char, String>) -> String {
        // First pass: caret escaping (^) and %VAR% / %0..%9 / %* / %%v
        let mut pass1 = String::with_capacity(input.len() + 16);
        let chars: Vec<char> = input.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '^' && i + 1 < chars.len() {
                pass1.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if chars[i] == '%' {
                // %%v or %v loop variable
                if i + 2 < chars.len() && chars[i + 1] == '%' && chars[i + 2].is_ascii_alphabetic() {
                    let vch = chars[i + 2];
                    if let Some(val) = loop_vars.get(&vch) {
                        pass1.push_str(val);
                        i += 3;
                        continue;
                    }
                }
                if i + 1 < chars.len() && chars[i + 1].is_ascii_alphabetic() && loop_vars.contains_key(&chars[i + 1]) {
                    let vch = chars[i + 1];
                    if (i + 2 >= chars.len() || chars[i + 2] != '%') && loop_vars.contains_key(&vch) {
                        pass1.push_str(&loop_vars[&vch]);
                        i += 2;
                        continue;
                    }
                }
                // %~dp0
                if i + 3 < chars.len() && chars[i + 1] == '~' {
                    let mut j = i + 2;
                    while j < chars.len() && chars[j].is_ascii_alphabetic() {
                        j += 1;
                    }
                    if j < chars.len() && chars[j].is_ascii_digit() {
                        let digit = (chars[j] as u8 - b'0') as usize;
                        let base_val = if digit == 0 {
                            self.cwd_win.clone() + "\\"
                        } else {
                            self.args.get(digit - 1).cloned().unwrap_or_default()
                        };
                        pass1.push_str(&base_val);
                        i = j + 1;
                        continue;
                    }
                }
                // %*
                if i + 1 < chars.len() && chars[i + 1] == '*' {
                    pass1.push_str(&self.args.join(" "));
                    i += 2;
                    continue;
                }
                // %0..%9
                if i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
                    let digit = (chars[i + 1] as u8 - b'0') as usize;
                    if digit == 0 {
                        pass1.push_str(&self.script_name);
                    } else if let Some(arg_val) = self.args.get(digit - 1) {
                        pass1.push_str(arg_val);
                    }
                    i += 2;
                    continue;
                }
                // %VAR%
                if let Some(rel_end) = chars[i + 1..].iter().position(|&c| c == '%') {
                    let end = i + 1 + rel_end;
                    let var_name: String = chars[i + 1..end].iter().collect();
                    if !var_name.is_empty() {
                        if let Some(val) = self.get_var_ci(&var_name) {
                            pass1.push_str(&val);
                        }
                        i = end + 1;
                        continue;
                    }
                }
            }
            pass1.push(chars[i]);
            i += 1;
        }

        if !self.delayed_expansion || !pass1.contains('!') {
            return pass1;
        }

        // Second pass: !VAR! delayed expansion
        let mut pass2 = String::with_capacity(pass1.len());
        let chars2: Vec<char> = pass1.chars().collect();
        let mut k = 0;
        while k < chars2.len() {
            if chars2[k] == '!' {
                if let Some(rel_end) = chars2[k + 1..].iter().position(|&c| c == '!') {
                    let end = k + 1 + rel_end;
                    let var_name: String = chars2[k + 1..end].iter().collect();
                    if !var_name.is_empty() && var_name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                        if let Some(val) = self.get_var_ci(&var_name) {
                            pass2.push_str(&val);
                        }
                        k = end + 1;
                        continue;
                    }
                }
            }
            pass2.push(chars2[k]);
            k += 1;
        }
        pass2
    }

    fn resolve_win_target(&self, raw: &str) -> std::path::PathBuf {
        let clean = raw.trim().trim_matches('"');
        if clean.len() >= 2 && clean.as_bytes()[1] == b':' {
            self.vfs.win_to_android_path(clean)
        } else if clean.starts_with('\\') || clean.starts_with('/') {
            let drv = &self.cwd_win[..2];
            self.vfs.win_to_android_path(&format!("{}{}", drv, clean))
        } else {
            let combined = format!("{}\\{}", self.cwd_win.trim_end_matches('\\'), clean);
            self.vfs.win_to_android_path(&combined)
        }
    }

    pub fn execute_script(&mut self, script_src: &str) -> BatchExecOutcome {
        let raw_lines: Vec<&str> = script_src.lines().collect();
        // Index :labels -> line index
        let mut labels: BTreeMap<String, usize> = BTreeMap::new();
        for (idx, l) in raw_lines.iter().enumerate() {
            let t = l.trim();
            if let Some(rest) = t.strip_prefix(':') {
                if !rest.starts_with(':') {
                    let lbl: String = rest
                        .trim()
                        .chars()
                        .take_while(|c| !c.is_whitespace())
                        .collect();
                    if !lbl.is_empty() {
                        labels.insert(lbl.to_ascii_lowercase(), idx);
                    }
                }
            }
        }

        let mut pc = 0;
        let mut loop_vars = BTreeMap::new();
        let max_steps = 5000usize;
        let mut steps = 0usize;

        while pc < raw_lines.len() {
            if self.is_cancelled() {
                self.stderr_lines.push("[CANCELLED] Execution stopped by user.".to_string());
                return self.finish(true);
            }
            steps += 1;
            if steps > max_steps {
                self.stderr_lines.push("[ERROR] Maximum step limit (5000) exceeded (infinite loop protection).".to_string());
                self.last_exit_code = 1;
                break;
            }

            let raw_line = raw_lines[pc].trim();
            let line_number = pc + 1;
            pc += 1;

            if raw_line.is_empty() || raw_line.starts_with("::") || raw_line.starts_with(':') {
                continue;
            }

            match self.exec_statement(raw_line, line_number, &mut loop_vars) {
                FlowControl::Continue => {}
                FlowControl::Goto(target) => {
                    let t_low = target.trim_start_matches(':').to_ascii_lowercase();
                    if t_low == "eof" {
                        break;
                    }
                    if let Some(&dest_pc) = labels.get(&t_low) {
                        pc = dest_pc + 1;
                    } else {
                        self.stderr_lines.push(format!(
                            "Line {}: The system cannot find the batch label specified - {}",
                            line_number, target
                        ));
                        self.last_exit_code = 1;
                        break;
                    }
                }
                FlowControl::Exit(code) => {
                    self.last_exit_code = code;
                    break;
                }
            }
        }

        self.finish(false)
    }

    fn finish(&self, cancelled: bool) -> BatchExecOutcome {
        BatchExecOutcome {
            stdout: self.stdout_lines.join("\n"),
            stderr: self.stderr_lines.join("\n"),
            exit_code: if cancelled { 130 } else { self.last_exit_code },
            final_cwd_win: self.cwd_win.clone(),
            title: self.title.clone(),
            cancelled,
        }
    }

    fn exec_statement(
        &mut self,
        raw_stmt: &str,
        line_number: usize,
        loop_vars: &mut BTreeMap<char, String>,
    ) -> FlowControl {
        let mut clean = raw_stmt.trim();
        if clean.is_empty() || clean.starts_with("::") {
            return FlowControl::Continue;
        }
        if let Some(stripped) = clean.strip_prefix('@') {
            clean = stripped.trim();
        }

        // Check for top-level command chaining (&&, ||, &) outside quotes and parentheses
        if let Some((left, op, right)) = split_chain_operator(clean) {
            let fc = self.exec_statement(left, line_number, loop_vars);
            if !matches!(fc, FlowControl::Continue) {
                return fc;
            }
            match op {
                ChainOp::And if self.last_exit_code == 0 => {
                    return self.exec_statement(right, line_number, loop_vars);
                }
                ChainOp::Or if self.last_exit_code != 0 => {
                    return self.exec_statement(right, line_number, loop_vars);
                }
                ChainOp::Seq => {
                    return self.exec_statement(right, line_number, loop_vars);
                }
                _ => return FlowControl::Continue,
            }
        }

        let expanded = self.expand_variables(clean, loop_vars);
        let trimmed = expanded.trim();
        let lower = trimmed.to_ascii_lowercase();

        if lower.starts_with("rem") && (lower.len() == 3 || lower.as_bytes()[3].is_ascii_whitespace()) {
            return FlowControl::Continue;
        }

        // Handle FOR loop before variable expansion strips %%v
        if clean.to_ascii_lowercase().starts_with("for ") {
            return self.exec_for_loop(clean, line_number, loop_vars);
        }

        // Handle IF / ELSE
        if lower.starts_with("if ") {
            return self.exec_if_statement(trimmed, line_number, loop_vars);
        }

        // Check output redirection (> or >>)
        let (cmd_core, redir_spec) = extract_redirection(trimmed);
        let mut captured_out = Vec::new();
        let fc = self.exec_single_command(&cmd_core, line_number, &mut captured_out);

        if let Some((target_file, append)) = redir_spec {
            let path_buf = self.resolve_win_target(&target_file);
            if let Some(p) = path_buf.parent() {
                let _ = fs::create_dir_all(p);
            }
            let text_out = captured_out.join("\n") + if captured_out.is_empty() { "" } else { "\n" };
            let write_res = if append {
                use std::io::Write;
                fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path_buf)
                    .and_then(|mut f| f.write_all(text_out.as_bytes()))
            } else {
                fs::write(&path_buf, text_out.as_bytes())
            };
            if let Err(e) = write_res {
                self.stderr_lines.push(format!("Line {}: Redirection error to {}: {}", line_number, target_file, e));
                self.last_exit_code = 1;
            }
        } else {
            self.stdout_lines.extend(captured_out);
        }

        fc
    }

    fn exec_for_loop(
        &mut self,
        raw_for: &str,
        line_number: usize,
        loop_vars: &mut BTreeMap<char, String>,
    ) -> FlowControl {
        let lower = raw_for.to_ascii_lowercase();
        let after_for = raw_for[4..].trim();
        if after_for.starts_with('/') {
            let opt = after_for.split_whitespace().next().unwrap_or("/F");
            self.stderr_lines.push(format!(
                "Batch syntax not supported at line {}: FOR {} options are not implemented in the MVP batch interpreter.",
                line_number, opt
            ));
            self.last_exit_code = 1;
            return FlowControl::Continue;
        }

        if let Some(in_pos) = lower.find(" in (") {
            let var_token = raw_for[4..in_pos].trim().trim_start_matches('%');
            let var_ch = var_token.chars().next().unwrap_or('i');
            let after_in = &raw_for[in_pos + 5..];
            if let Some(close_paren) = after_in.find(')') {
                let list_raw = &after_in[..close_paren];
                let list_expanded = self.expand_variables(list_raw, loop_vars);
                let after_paren = after_in[close_paren + 1..].trim();
                let do_body = if after_paren.len() >= 3 && after_paren[..3].eq_ignore_ascii_case("do ") {
                    after_paren[3..].trim()
                } else {
                    after_paren
                };

                for item in list_expanded.split(|c: char| c.is_whitespace() || c == ',' || c == ';').filter(|s| !s.is_empty()) {
                    loop_vars.insert(var_ch, item.trim_matches('"').to_string());
                    let fc = self.exec_statement(do_body, line_number, loop_vars);
                    if !matches!(fc, FlowControl::Continue) {
                        loop_vars.remove(&var_ch);
                        return fc;
                    }
                }
                loop_vars.remove(&var_ch);
                return FlowControl::Continue;
            }
        }

        self.stderr_lines.push(format!("Line {}: Malformed FOR syntax: {}", line_number, raw_for));
        self.last_exit_code = 1;
        FlowControl::Continue
    }

    fn exec_if_statement(
        &mut self,
        stmt: &str,
        line_number: usize,
        loop_vars: &mut BTreeMap<char, String>,
    ) -> FlowControl {
        let mut rest = stmt[3..].trim();
        let mut case_insensitive = false;
        if rest.len() >= 3 && rest[..3].eq_ignore_ascii_case("/i ") {
            case_insensitive = true;
            rest = rest[3..].trim();
        }
        let mut negate = false;
        if rest.len() >= 4 && rest[..4].eq_ignore_ascii_case("not ") {
            negate = true;
            rest = rest[4..].trim();
        }

        let (cond_met, remainder) = if rest.len() >= 6 && rest[..6].eq_ignore_ascii_case("exist ") {
            let after_exist = rest[6..].trim();
            if let Some((path_tok, tail)) = split_first_arg(after_exist) {
                let resolved = self.resolve_win_target(path_tok);
                (resolved.exists(), tail)
            } else {
                (false, "")
            }
        } else if rest.len() >= 11 && rest[..11].eq_ignore_ascii_case("errorlevel ") {
            let after_err = rest[11..].trim();
            if let Some((lvl_tok, tail)) = split_first_arg(after_err) {
                let lvl = lvl_tok.parse::<i32>().unwrap_or(0);
                (self.last_exit_code >= lvl, tail)
            } else {
                (false, "")
            }
        } else if let Some(eq_pos) = rest.find("==") {
            let lhs = rest[..eq_pos].trim().trim_matches('"');
            let after_eq = rest[eq_pos + 2..].trim();
            if let Some((rhs_tok, tail)) = split_first_arg(after_eq) {
                let rhs = rhs_tok.trim_matches('"');
                let matched = if case_insensitive {
                    lhs.eq_ignore_ascii_case(rhs)
                } else {
                    lhs == rhs
                };
                (matched, tail)
            } else {
                (false, "")
            }
        } else {
            // Word comparison: IF A EQU B / NEQ / LSS / LEQ / GTR / GEQ
            if let Some((lhs_tok, after_lhs)) = split_first_arg(rest) {
                if let Some((op_tok, after_op)) = split_first_arg(after_lhs) {
                    if let Some((rhs_tok, tail)) = split_first_arg(after_op) {
                        let l_str = lhs_tok.trim_matches('"');
                        let r_str = rhs_tok.trim_matches('"');
                        let l_num = l_str.parse::<i64>();
                        let r_num = r_str.parse::<i64>();
                        let op_up = op_tok.to_ascii_uppercase();
                        let res = match (l_num, r_num, op_up.as_str()) {
                            (Ok(a), Ok(b), "EQU") => a == b,
                            (Ok(a), Ok(b), "NEQ") => a != b,
                            (Ok(a), Ok(b), "LSS") => a < b,
                            (Ok(a), Ok(b), "LEQ") => a <= b,
                            (Ok(a), Ok(b), "GTR") => a > b,
                            (Ok(a), Ok(b), "GEQ") => a >= b,
                            (_, _, "EQU") => if case_insensitive { l_str.eq_ignore_ascii_case(r_str) } else { l_str == r_str },
                            (_, _, "NEQ") => if case_insensitive { !l_str.eq_ignore_ascii_case(r_str) } else { l_str != r_str },
                            _ => false,
                        };
                        (res, tail)
                    } else {
                        (false, "")
                    }
                } else {
                    (false, "")
                }
            } else {
                (false, "")
            }
        };

        let final_cond = if negate { !cond_met } else { cond_met };
        let (then_branch, else_branch) = split_else_branch(remainder);

        if final_cond {
            self.exec_statement(then_branch, line_number, loop_vars)
        } else if let Some(eb) = else_branch {
            self.exec_statement(eb, line_number, loop_vars)
        } else {
            FlowControl::Continue
        }
    }

    fn exec_single_command(
        &mut self,
        cmd_str: &str,
        line_number: usize,
        out_buf: &mut Vec<String>,
    ) -> FlowControl {
        let trimmed = cmd_str.trim();
        if trimmed.is_empty() {
            return FlowControl::Continue;
        }

        // Drive switch C:, D:, Z:
        if trimmed.len() == 2 && trimmed.as_bytes()[1] == b':' && trimmed.as_bytes()[0].is_ascii_alphabetic() {
            let drv = (trimmed.as_bytes()[0] as char).to_ascii_uppercase();
            self.cwd_win = format!("{}:\\", drv);
            self.last_exit_code = 0;
            return FlowControl::Continue;
        }

        let lower = trimmed.to_ascii_lowercase();
        if lower == "echo off" {
            self.echo_on = false;
            return FlowControl::Continue;
        }
        if lower == "echo on" {
            self.echo_on = true;
            return FlowControl::Continue;
        }
        if lower == "echo." || lower == "echo:" {
            out_buf.push(String::new());
            return FlowControl::Continue;
        }
        if lower == "echo" {
            out_buf.push(format!("ECHO is {}.", if self.echo_on { "on" } else { "off" }));
            return FlowControl::Continue;
        }
        if lower.starts_with("echo ") {
            out_buf.push(trimmed[5..].to_string());
            self.last_exit_code = 0;
            return FlowControl::Continue;
        }

        let (cmd_word, rest_args) = split_first_arg(trimmed).unwrap_or((trimmed, ""));
        let cmd_low = cmd_word.to_ascii_lowercase();
        let cmd_clean = cmd_low.strip_suffix(".exe").unwrap_or(&cmd_low);

        match cmd_clean {
            "cls" | "clear" => {
                self.stdout_lines.clear();
                self.stderr_lines.clear();
                self.last_exit_code = 0;
            }
            "title" => {
                self.title = Some(rest_args.trim().to_string());
                self.last_exit_code = 0;
            }
            "ver" => {
                out_buf.push("Microsoft Windows [Version 10.0.22631.4317 - Rust VFS Batch Engine v4.0]".to_string());
                self.last_exit_code = 0;
            }
            "help" => {
                out_buf.push("Supported Windows Batch / CMD Commands:".to_string());
                out_buf.push("  ECHO, SET, SET /A, SETLOCAL, ENDLOCAL, CD/CHDIR, DIR, CLS, TYPE, COPY, MOVE".to_string());
                out_buf.push("  DEL/ERASE, MD/MKDIR, RD/RMDIR, REN/RENAME, IF/ELSE, FOR, GOTO, CALL, SHIFT".to_string());
                out_buf.push("  EXIT /B, PAUSE, TITLE, PATH, WHERE, VER, FIND, FINDSTR, IPCONFIG, SYSTEMINFO".to_string());
                self.last_exit_code = 0;
            }
            "pause" => {
                out_buf.push("Press any key to continue . . .".to_string());
                self.last_exit_code = 0;
            }
            "setlocal" => {
                self.env_stack.push((self.env_map.clone(), self.delayed_expansion));
                if rest_args.to_ascii_lowercase().contains("enabledelayedexpansion") {
                    self.delayed_expansion = true;
                } else if rest_args.to_ascii_lowercase().contains("disabledelayedexpansion") {
                    self.delayed_expansion = false;
                }
                self.last_exit_code = 0;
            }
            "endlocal" => {
                if let Some((prev_env, prev_del)) = self.env_stack.pop() {
                    self.env_map = prev_env;
                    self.delayed_expansion = prev_del;
                }
                self.last_exit_code = 0;
            }
            "shift" => {
                if !self.args.is_empty() {
                    self.args.remove(0);
                }
                self.last_exit_code = 0;
            }
            "path" => {
                if rest_args.trim().is_empty() {
                    let p = self.get_var_ci("PATH").unwrap_or_default();
                    out_buf.push(format!("PATH={}", p));
                } else {
                    let val = rest_args.trim().trim_start_matches('=').trim();
                    self.set_var_ci("PATH", val.to_string());
                }
                self.last_exit_code = 0;
            }
            "set" => {
                let r = rest_args.trim();
                if r.is_empty() {
                    for (k, v) in &self.env_map {
                        out_buf.push(format!("{}={}", k, v));
                    }
                    self.last_exit_code = 0;
                } else if r.len() >= 2 && r[..2].eq_ignore_ascii_case("/a") {
                    let expr = r[2..].trim();
                    if let Some((var, rhs)) = expr.split_once('=') {
                        let vname = var.trim();
                        let val = eval_arith(rhs.trim(), &self.env_map);
                        self.set_var_ci(vname, val.to_string());
                        self.last_exit_code = 0;
                    } else {
                        let val = eval_arith(expr, &self.env_map);
                        out_buf.push(val.to_string());
                        self.last_exit_code = 0;
                    }
                } else if r.len() >= 2 && r[..2].eq_ignore_ascii_case("/p") {
                    let expr = r[2..].trim();
                    if let Some((var, prompt_str)) = expr.split_once('=') {
                        out_buf.push(format!("{}", prompt_str.trim().trim_matches('"')));
                        self.set_var_ci(var.trim(), String::new());
                    }
                    self.last_exit_code = 0;
                } else if let Some((var, val)) = r.split_once('=') {
                    let vname = var.trim().trim_matches('"');
                    let vval = val.trim().trim_matches('"').to_string();
                    self.set_var_ci(vname, vval);
                    self.last_exit_code = 0;
                } else {
                    if let Some(v) = self.get_var_ci(r) {
                        out_buf.push(format!("{}={}", r, v));
                        self.last_exit_code = 0;
                    } else {
                        self.stderr_lines.push(format!("Environment variable {} not defined", r));
                        self.last_exit_code = 1;
                    }
                }
            }
            "cd" | "chdir" => {
                let mut r = rest_args.trim();
                if r.len() >= 2 && r[..2].eq_ignore_ascii_case("/d") {
                    r = r[2..].trim();
                }
                if r.is_empty() {
                    out_buf.push(self.cwd_win.clone());
                    self.last_exit_code = 0;
                } else if r == ".." {
                    let p = self.resolve_win_target(&self.cwd_win);
                    if let Some(parent) = p.parent() {
                        self.cwd_win = self.vfs.android_to_win_path(parent);
                    }
                    self.last_exit_code = 0;
                } else {
                    let target = self.resolve_win_target(r);
                    if target.exists() && target.is_dir() {
                        self.cwd_win = self.vfs.android_to_win_path(&target);
                        self.last_exit_code = 0;
                    } else {
                        self.stderr_lines.push(format!("The system cannot find the path specified: {}", r));
                        self.last_exit_code = 1;
                    }
                }
            }
            "md" | "mkdir" => {
                let target_arg = rest_args.trim().trim_matches('"');
                if target_arg.is_empty() {
                    self.stderr_lines.push("The syntax of the command is incorrect.".to_string());
                    self.last_exit_code = 1;
                } else {
                    let p = self.resolve_win_target(target_arg);
                    match fs::create_dir_all(&p) {
                        Ok(_) => self.last_exit_code = 0,
                        Err(e) => {
                            self.stderr_lines.push(format!("mkdir failed for {}: {}", target_arg, e));
                            self.last_exit_code = 1;
                        }
                    }
                }
            }
            "rd" | "rmdir" => {
                let mut r = rest_args.trim();
                loop {
                    let rl = r.to_ascii_lowercase();
                    if rl.starts_with("/s") || rl.starts_with("/q") {
                        r = r[2..].trim();
                    } else {
                        break;
                    }
                }
                let p = self.resolve_win_target(r);
                if p.exists() {
                    let _ = fs::remove_dir_all(&p);
                    self.last_exit_code = 0;
                } else {
                    self.stderr_lines.push(format!("The system cannot find the file specified: {}", r));
                    self.last_exit_code = 2;
                }
            }
            "del" | "erase" => {
                let mut r = rest_args.trim();
                loop {
                    let rl = r.to_ascii_lowercase();
                    if rl.starts_with("/f") || rl.starts_with("/q") || rl.starts_with("/s") {
                        r = r[2..].trim();
                    } else {
                        break;
                    }
                }
                let p = self.resolve_win_target(r);
                if p.exists() && p.is_file() {
                    let _ = fs::remove_file(&p);
                    self.last_exit_code = 0;
                } else {
                    self.stderr_lines.push(format!("Could Not Find {}", r));
                    self.last_exit_code = 1;
                }
            }
            "type" => {
                let p = self.resolve_win_target(rest_args.trim());
                match fs::read_to_string(&p) {
                    Ok(txt) => {
                        for l in txt.lines() {
                            out_buf.push(l.to_string());
                        }
                        self.last_exit_code = 0;
                    }
                    Err(_) => {
                        self.stderr_lines.push(format!("The system cannot find the file specified: {}", rest_args.trim()));
                        self.last_exit_code = 1;
                    }
                }
            }
            "copy" => {
                let mut r = rest_args.trim();
                if r.len() >= 2 && r[..2].eq_ignore_ascii_case("/y") {
                    r = r[2..].trim();
                }
                if let Some((src_arg, dst_arg)) = split_first_arg(r) {
                    let src_p = self.resolve_win_target(src_arg);
                    let mut dst_p = self.resolve_win_target(dst_arg);
                    if dst_p.is_dir() {
                        if let Some(fname) = src_p.file_name() {
                            dst_p = dst_p.join(fname);
                        }
                    }
                    match fs::copy(&src_p, &dst_p) {
                        Ok(_) => {
                            out_buf.push("        1 file(s) copied.".to_string());
                            self.last_exit_code = 0;
                        }
                        Err(e) => {
                            self.stderr_lines.push(format!("Copy failed: {}", e));
                            self.last_exit_code = 1;
                        }
                    }
                } else {
                    self.stderr_lines.push("The syntax of the command is incorrect.".to_string());
                    self.last_exit_code = 1;
                }
            }
            "move" | "ren" | "rename" => {
                let mut r = rest_args.trim();
                if r.len() >= 2 && r[..2].eq_ignore_ascii_case("/y") {
                    r = r[2..].trim();
                }
                if let Some((src_arg, dst_arg)) = split_first_arg(r) {
                    let src_p = self.resolve_win_target(src_arg);
                    let dst_p = self.resolve_win_target(dst_arg);
                    match fs::rename(&src_p, &dst_p) {
                        Ok(_) => {
                            out_buf.push("        1 file(s) moved.".to_string());
                            self.last_exit_code = 0;
                        }
                        Err(e) => {
                            self.stderr_lines.push(format!("Move/Rename failed: {}", e));
                            self.last_exit_code = 1;
                        }
                    }
                } else {
                    self.stderr_lines.push("The syntax of the command is incorrect.".to_string());
                    self.last_exit_code = 1;
                }
            }
            "dir" => {
                let clean_arg = rest_args
                    .split_whitespace()
                    .find(|t| !t.starts_with('/'))
                    .unwrap_or("");
                let dir_p = if clean_arg.is_empty() {
                    self.resolve_win_target(&self.cwd_win)
                } else {
                    self.resolve_win_target(clean_arg)
                };
                let win_disp = self.vfs.android_to_win_path(&dir_p);
                out_buf.push(format!(" Directory of {}", win_disp));
                out_buf.push(String::new());
                if let Ok(entries) = fs::read_dir(&dir_p) {
                    let mut count_files = 0;
                    let mut count_dirs = 0;
                    for e in entries.flatten() {
                        let name = e.file_name().to_string_lossy().into_owned();
                        let meta = e.metadata().ok();
                        let is_d = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
                        let sz = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                        if is_d {
                            count_dirs += 1;
                            out_buf.push(format!("    <DIR>          {}", name));
                        } else {
                            count_files += 1;
                            out_buf.push(format!("{:>14} B   {}", sz, name));
                        }
                    }
                    out_buf.push(format!("               {} File(s), {} Dir(s)", count_files, count_dirs));
                    self.last_exit_code = 0;
                } else {
                    self.stderr_lines.push(format!("File Not Found: {}", win_disp));
                    self.last_exit_code = 1;
                }
            }
            "where" => {
                let target = rest_args.trim();
                let sys32_cand = self.vfs.drive_c.join("Windows/System32").join(target);
                if sys32_cand.exists() {
                    out_buf.push(self.vfs.android_to_win_path(&sys32_cand));
                    self.last_exit_code = 0;
                } else {
                    out_buf.push(format!("INFO: Could not find files for the given pattern(s): {}", target));
                    self.last_exit_code = 1;
                }
            }
            "find" | "findstr" => {
                let mut ignore_case = false;
                let mut toks = Vec::new();
                for t in rest_args.split_whitespace() {
                    if t.eq_ignore_ascii_case("/i") {
                        ignore_case = true;
                    } else if !t.starts_with('/') {
                        toks.push(t.trim_matches('"'));
                    }
                }
                if toks.len() >= 2 {
                    let pat = toks[0];
                    let file_p = self.resolve_win_target(toks[1]);
                    if let Ok(content) = fs::read_to_string(&file_p) {
                        let mut matched = false;
                        for line in content.lines() {
                            let hit = if ignore_case {
                                line.to_ascii_lowercase().contains(&pat.to_ascii_lowercase())
                            } else {
                                line.contains(pat)
                            };
                            if hit {
                                out_buf.push(line.to_string());
                                matched = true;
                            }
                        }
                        self.last_exit_code = if matched { 0 } else { 1 };
                    } else {
                        self.stderr_lines.push(format!("FINDSTR: Cannot open {}", toks[1]));
                        self.last_exit_code = 2;
                    }
                } else {
                    self.stderr_lines.push("FINDSTR: Missing pattern or filename.".to_string());
                    self.last_exit_code = 2;
                }
            }
            "goto" => {
                let target = rest_args.trim();
                return FlowControl::Goto(target.to_string());
            }
            "call" => {
                let target = rest_args.trim();
                if target.starts_with(':') {
                    return FlowControl::Goto(target.to_string());
                }
                let (sub_script, sub_args_raw) = split_first_arg(target).unwrap_or((target, ""));
                let mut script_p = self.resolve_win_target(sub_script);
                if !script_p.exists() && !sub_script.ends_with(".bat") && !sub_script.ends_with(".cmd") {
                    let try_bat = self.resolve_win_target(&format!("{}.bat", sub_script));
                    if try_bat.exists() {
                        script_p = try_bat;
                    }
                }
                if let Ok(sub_src) = fs::read_to_string(&script_p) {
                    let prev_args = self.args.clone();
                    self.args = sub_args_raw.split_whitespace().map(|s| s.to_string()).collect();
                    let sub_res = self.execute_script(&sub_src);
                    self.args = prev_args;
                    self.last_exit_code = sub_res.exit_code;
                } else {
                    self.stderr_lines.push(format!("Line {}: CALL target not found: {}", line_number, sub_script));
                    self.last_exit_code = 1;
                }
            }
            "exit" => {
                let mut r = rest_args.trim();
                if r.len() >= 2 && r[..2].eq_ignore_ascii_case("/b") {
                    r = r[2..].trim();
                }
                let code = r.parse::<i32>().unwrap_or(self.last_exit_code);
                return FlowControl::Exit(code);
            }
            _ => {
                // Report clear line-specific diagnostic for unimplemented or external Windows-only commands
                self.stderr_lines.push(format!(
                    "Line {}: '{}' is not implemented in the built-in Windows Batch interpreter or requires an external Windows binary runtime.",
                    line_number, cmd_word
                ));
                self.last_exit_code = 9009;
            }
        }

        FlowControl::Continue
    }
}

enum FlowControl {
    Continue,
    Goto(String),
    Exit(i32),
}

#[derive(Clone, Copy)]
enum ChainOp {
    And,
    Or,
    Seq,
}

fn split_chain_operator(s: &str) -> Option<(&str, ChainOp, &str)> {
    let bytes = s.as_bytes();
    let mut in_quotes = false;
    let mut paren_depth = 0i32;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'^' => {
                i += 2;
                continue;
            }
            b'"' => in_quotes = !in_quotes,
            b'(' if !in_quotes => paren_depth += 1,
            b')' if !in_quotes && paren_depth > 0 => paren_depth -= 1,
            b'&' if !in_quotes && paren_depth == 0 => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'&' {
                    return Some((&s[..i], ChainOp::And, &s[i + 2..]));
                } else {
                    return Some((&s[..i], ChainOp::Seq, &s[i + 1..]));
                }
            }
            b'|' if !in_quotes && paren_depth == 0 => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'|' {
                    return Some((&s[..i], ChainOp::Or, &s[i + 2..]));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn extract_redirection(s: &str) -> (String, Option<(String, bool)>) {
    let bytes = s.as_bytes();
    let mut in_quotes = false;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            in_quotes = !in_quotes;
        } else if !in_quotes && bytes[i] == b'>' {
            let is_append = i + 1 < bytes.len() && bytes[i + 1] == b'>';
            let cmd_part = s[..i].trim().to_string();
            let file_part = if is_append { &s[i + 2..] } else { &s[i + 1..] }
                .trim()
                .trim_matches('"')
                .to_string();
            if !file_part.is_empty() {
                return (cmd_part, Some((file_part, is_append)));
            }
        }
        i += 1;
    }
    (s.to_string(), None)
}

fn split_first_arg(s: &str) -> Option<(&str, &str)> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(stripped) = s.strip_prefix('"') {
        if let Some(end_q) = stripped.find('"') {
            let tok = &s[..end_q + 2];
            let rest = s[end_q + 2..].trim();
            return Some((tok, rest));
        }
    }
    let mut it = s.splitn(2, char::is_whitespace);
    let first = it.next()?;
    let rest = it.next().unwrap_or("").trim();
    Some((first, rest))
}

fn split_else_branch(s: &str) -> (&str, Option<&str>) {
    let lower = s.to_ascii_lowercase();
    if let Some(idx) = lower.find(" else ") {
        (&s[..idx].trim(), Some(s[idx + 6..].trim()))
    } else {
        (s.trim(), None)
    }
}

fn eval_arith(expr: &str, env_map: &BTreeMap<String, String>) -> i64 {
    let expr = expr.trim();
    if let Some((a, b)) = expr.split_once('+') {
        return eval_arith(a, env_map) + eval_arith(b, env_map);
    }
    if let Some((a, b)) = expr.rsplit_once('-') {
        if !a.trim().is_empty() {
            return eval_arith(a, env_map) - eval_arith(b, env_map);
        }
    }
    if let Some((a, b)) = expr.split_once('*') {
        return eval_arith(a, env_map) * eval_arith(b, env_map);
    }
    if let Some((a, b)) = expr.split_once('/') {
        let denom = eval_arith(b, env_map);
        return if denom != 0 { eval_arith(a, env_map) / denom } else { 0 };
    }
    if let Ok(n) = expr.parse::<i64>() {
        return n;
    }
    for (k, v) in env_map {
        if k.eq_ignore_ascii_case(expr) {
            return v.parse::<i64>().unwrap_or(0);
        }
    }
    0
}
