//! Zero-dependency Rust Windows `.reg` parser and Linux environment variable applicator.

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct RegReport {
    pub file_path: String,
    pub applied: Vec<(String, String)>,
    pub skipped: Vec<String>,
}

pub fn parse_and_apply_reg(content: &str, file_label: &str, write_bashrc: bool) -> RegReport {
    let mut current_key: Option<String> = None;
    let mut applied = Vec::new();
    let mut skipped = Vec::new();

    let home_dir = env::var("HOME").unwrap_or_else(|_| "/data/data/com.termux/files/home".to_string());
    let bashrc_path = PathBuf::from(&home_dir).join(".bashrc");
    let mut existing_bashrc = if write_bashrc && bashrc_path.exists() {
        fs::read_to_string(&bashrc_path).unwrap_or_default()
    } else {
        String::new()
    };

    for raw_line in content.lines() {
        let line = raw_line.trim().trim_start_matches('\u{feff}');
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
                let raw_name = line[..eq_idx].trim().trim_matches('"');
                let raw_val = line[eq_idx + 1..].trim();

                let key_norm = key.to_ascii_lowercase();
                let is_env_key = key_norm.contains("hkey_current_user\\environment")
                    || key_norm.contains("hkcu\\environment")
                    || key_norm.contains("currentcontrolset\\control\\session manager\\environment");

                let val_lower = raw_val.to_ascii_lowercase();
                let is_binary_or_int = val_lower.starts_with("hex:")
                    || val_lower.starts_with("dword:")
                    || val_lower.starts_with("qword:")
                    || val_lower.starts_with("hex(");

                if is_env_key && !is_binary_or_int && raw_name != "@" {
                    let clean_val = raw_val
                        .trim_matches('"')
                        .replace("\\\\", "/")
                        .replace("%SystemRoot%", "/system")
                        .replace("%USERPROFILE%", &home_dir)
                        .replace("%TEMP%", "/tmp")
                        .replace("%TMP%", "/tmp");

                    let export_line = format!("export {}=\"{}\"", raw_name, clean_val);
                    if write_bashrc && !existing_bashrc.contains(&export_line) {
                        if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&bashrc_path) {
                            let _ = writeln!(f, "\n{}", export_line);
                            existing_bashrc.push('\n');
                            existing_bashrc.push_str(&export_line);
                        }
                    }
                    applied.push((raw_name.to_string(), clean_val));
                } else {
                    skipped.push(format!("[{}] {} (Windows-only or binary)", key, raw_name));
                }
            }
        }
    }

    RegReport {
        file_path: file_label.to_string(),
        applied,
        skipped,
    }
}

pub fn format_reg_report(report: &RegReport) -> String {
    let mut out = Vec::new();
    out.push("=== .reg Parse Report (Rust Engine) ===".to_string());
    out.push(format!("File: {}", report.file_path));
    out.push(format!("\nApplied to ~/.bashrc ({}):", report.applied.len()));
    if report.applied.is_empty() {
        out.push("  (none)".to_string());
    } else {
        for (k, v) in &report.applied {
            out.push(format!("  {} = {}", k, v));
        }
    }
    out.push(format!("\nSkipped - Windows-only ({}):", report.skipped.len()));
    if report.skipped.is_empty() {
        out.push("  (none)".to_string());
    } else {
        for s in report.skipped.iter().take(10) {
            out.push(format!("  {}", s));
        }
        if report.skipped.len() > 10 {
            out.push(format!("  ...and {} more", report.skipped.len() - 10));
        }
    }
    out.push("\nRun: source ~/.bashrc  to reload env vars".to_string());
    out.join("\n")
}

pub fn read_reg_file(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    // Handle UTF-16LE (common in Windows regedit exports) or UTF-8
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
