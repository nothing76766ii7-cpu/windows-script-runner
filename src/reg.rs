//! Windows `.reg` Parser, Validator, Previewer, and Isolated Virtual Registry Store.
//! Never alters Android system files; stores registry keys inside `~/.wsr_vfs/virtual_registry.txt`
//! and maps `HKCU\Environment` variables into the VFS Batch session environment.

use crate::vfs::WinVfs;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct RegEntry {
    pub key_path: String,
    pub name: String,
    pub value: String,
    pub value_type: String, // "REG_SZ", "REG_DWORD", "REG_BINARY", "ENV_VAR"
    pub is_env_var: bool,
}

#[derive(Debug, Clone)]
pub struct RegReport {
    pub file_path: String,
    pub applied_mode: bool,
    pub entries: Vec<RegEntry>,
    pub env_vars: Vec<(String, String)>,
    pub warnings: Vec<String>,
}

pub fn parse_and_apply_reg(content: &str, file_label: &str, apply_to_vfs: bool) -> RegReport {
    let vfs = WinVfs::new();
    let _ = vfs.ensure_initialized();
    let mut current_key: Option<String> = None;
    let mut entries = Vec::new();
    let mut env_vars = Vec::new();
    let mut warnings = Vec::new();

    for (idx, raw_line) in content.lines().enumerate() {
        let line = raw_line.trim().trim_start_matches('\u{feff}');
        if line.is_empty()
            || line.starts_with(';')
            || line.starts_with("Windows Registry Editor")
            || line.starts_with("REGEDIT4")
        {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            let k = line[1..line.len() - 1].trim().to_string();
            if k.to_ascii_lowercase().contains("\\currentversion\\run") {
                warnings.push(format!(
                    "Line {}: Startup key [{}] will be recorded in virtual registry only (autorun is disabled for security).",
                    idx + 1,
                    k
                ));
            }
            current_key = Some(k);
            continue;
        }

        if let Some(ref key) = current_key {
            if let Some(eq_idx) = line.find('=') {
                let raw_name = line[..eq_idx].trim().trim_matches('"').to_string();
                let raw_val = line[eq_idx + 1..].trim();
                let val_low = raw_val.to_ascii_lowercase();

                let value_type = if val_low.starts_with("dword:") {
                    "REG_DWORD"
                } else if val_low.starts_with("qword:") {
                    "REG_QWORD"
                } else if val_low.starts_with("hex:") || val_low.starts_with("hex(") {
                    "REG_BINARY"
                } else {
                    "REG_SZ"
                };

                let key_low = key.to_ascii_lowercase();
                let is_env_key = key_low.contains("hkcu\\environment")
                    || key_low.contains("hkey_current_user\\environment")
                    || key_low.contains("currentcontrolset\\control\\session manager\\environment");

                let clean_val = raw_val
                    .trim_matches('"')
                    .replace("\\\\", "\\");

                let is_env = is_env_key && value_type == "REG_SZ" && raw_name != "@";
                if is_env {
                    env_vars.push((raw_name.clone(), clean_val.clone()));
                }

                entries.push(RegEntry {
                    key_path: key.clone(),
                    name: raw_name,
                    value: clean_val,
                    value_type: value_type.to_string(),
                    is_env_var: is_env,
                });
            }
        }
    }

    if apply_to_vfs {
        let reg_store_path = vfs.root_base.join("virtual_registry.conf");
        let mut map = load_virtual_registry(&reg_store_path);
        for e in &entries {
            let full_k = format!("{}\\{}", e.key_path, e.name);
            map.insert(full_k, format!("{}:{}", e.value_type, e.value));
        }
        let _ = save_virtual_registry(&reg_store_path, &map);

        let env_store_path = vfs.root_base.join("vfs_env.conf");
        let mut env_map = load_virtual_registry(&env_store_path);
        for (k, v) in &env_vars {
            env_map.insert(k.clone(), v.clone());
        }
        let _ = save_virtual_registry(&env_store_path, &env_map);
    }

    RegReport {
        file_path: file_label.to_string(),
        applied_mode: apply_to_vfs,
        entries,
        env_vars,
        warnings,
    }
}

pub fn load_vfs_env_vars(vfs: &WinVfs) -> BTreeMap<String, String> {
    let env_store_path = vfs.root_base.join("vfs_env.conf");
    load_virtual_registry(&env_store_path)
}

fn load_virtual_registry(path: &Path) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    if let Ok(txt) = fs::read_to_string(path) {
        for line in txt.lines() {
            if let Some((k, v)) = line.split_once('=') {
                map.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
    }
    map
}

fn save_virtual_registry(path: &Path, map: &BTreeMap<String, String>) -> std::io::Result<()> {
    let mut out = String::new();
    for (k, v) in map {
        out.push_str(&format!("{}={}\n", k, v));
    }
    fs::write(path, out)
}

pub fn format_reg_report(report: &RegReport) -> String {
    let mut out = Vec::new();
    let mode_str = if report.applied_mode {
        "APPLIED TO ISOLATED VFS REGISTRY"
    } else {
        "PREVIEW ONLY (Not yet applied)"
    };
    out.push(format!("=== Windows Registry (.reg) Report [{}] ===", mode_str));
    out.push(format!("File: {}", report.file_path));
    out.push(format!("Total Keys Parsed: {}", report.entries.len()));

    out.push(format!("\nEnvironment Variables ({}):", report.env_vars.len()));
    if report.env_vars.is_empty() {
        out.push("  (none)".to_string());
    } else {
        for (k, v) in &report.env_vars {
            out.push(format!("  [ENV] {} = \"{}\"", k, v));
        }
    }

    out.push(format!("\nRegistry Keys ({}):", report.entries.len()));
    for e in report.entries.iter().take(20) {
        out.push(format!("  [{}] \"{}\" ({}) = {}", e.key_path, e.name, e.value_type, e.value));
    }
    if report.entries.len() > 20 {
        out.push(format!("  ...and {} more entries", report.entries.len() - 20));
    }

    if !report.warnings.is_empty() {
        out.push("\nSecurity Notices:".to_string());
        for w in &report.warnings {
            out.push(format!("  * {}", w));
        }
    }

    out.join("\n")
}

pub fn read_reg_file(path: &Path) -> Result<String, String> {
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
