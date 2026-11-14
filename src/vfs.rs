//! Windows Virtual File System (`C:\`, `D:\`, `Z:\`), path resolver, and sandboxed file manager.
//! Exposes logical drives with transparent Android backing storage descriptions and strict path traversal protection.

use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone)]
pub struct WinVfs {
    pub root_base: PathBuf,
    pub drive_c: PathBuf,
    pub drive_d: PathBuf,
    pub drive_z: PathBuf,
}

impl WinVfs {
    pub fn new() -> Self {
        let base = env::var("WSR_VFS_ROOT")
            .ok()
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let home = env::var("HOME").unwrap_or_else(|_| "/data/user/0/com.vanta.wsr/files".to_string());
                PathBuf::from(home).join(".wsr_vfs")
            });
        let drive_c = base.join("drive_c");

        let drive_d = env::var("WSR_SAF_DIR")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                if Path::new("/storage/emulated/0/Download").exists() && Self::is_dir_readable(Path::new("/storage/emulated/0")) {
                    PathBuf::from("/storage/emulated/0")
                } else if Path::new("/sdcard").exists() && Self::is_dir_readable(Path::new("/sdcard")) {
                    PathBuf::from("/sdcard")
                } else {
                    base.join("drive_d")
                }
            });

        let drive_z = base.join("drive_z");

        Self {
            root_base: base,
            drive_c,
            drive_d,
            drive_z,
        }
    }

    fn is_dir_readable(p: &Path) -> bool {
        fs::read_dir(p).is_ok()
    }

    /// Initializes the isolated Windows VFS directories and starter `.bat`, `.cmd`, and `.reg` fixtures.
    /// Does NOT create fake `.exe` shell scripts inside `C:\Windows\System32`.
    pub fn ensure_initialized(&self) -> Result<(), String> {
        let dirs = [
            self.drive_c.join("Windows/System32/drivers/etc"),
            self.drive_c.join("Windows/SysWOW64"),
            self.drive_c.join("Program Files/Windows Script Runner"),
            self.drive_c.join("Program Files (x86)"),
            self.drive_c.join("ProgramData"),
            self.drive_c.join("Users/Admin/Desktop"),
            self.drive_c.join("Users/Admin/Documents"),
            self.drive_c.join("Users/Admin/Downloads"),
            self.drive_c.join("Users/Admin/AppData/Local/Temp"),
            self.drive_c.join("Users/Admin/AppData/Roaming"),
            self.drive_c.join("Temp"),
            self.root_base.join("drive_d/Downloads"),
            self.drive_z.join("workspace"),
            self.drive_z.join("bin"),
        ];

        for d in &dirs {
            fs::create_dir_all(d).map_err(|e| format!("Failed creating {}: {}", d.display(), e))?;
        }

        // Remove any legacy non-PE shell scripts from System32 so PE analysis is 100% honest
        let sys32 = self.drive_c.join("Windows/System32");
        for legacy in &["ipconfig", "tasklist", "systeminfo", "where"] {
            let p = sys32.join(legacy);
            if p.exists() {
                if let Ok(bytes) = fs::read(&p) {
                    if bytes.starts_with(b"#!/") {
                        let _ = fs::remove_file(&p);
                    }
                }
            }
        }

        // Seed a realistic Windows hosts file in System32\drivers\etc\hosts
        let hosts_file = sys32.join("drivers/etc/hosts");
        if !hosts_file.exists() {
            let _ = fs::write(
                &hosts_file,
                "# Windows VFS Local Hosts File (Isolated App Sandbox)\r\n127.0.0.1       localhost\r\n::1             localhost\r\n",
            );
        }

        // Seed starter scripts in C:\Users\Admin\Desktop
        let desktop = self.drive_c.join("Users/Admin/Desktop");
        let sample_bat = desktop.join("sys_check.bat");
        if !sample_bat.exists() {
            let bat_content = "@echo off\r\n\
                title Windows Script Runner VFS Diagnostics\r\n\
                echo ================================================\r\n\
                echo Running inside Windows VFS Drive C:\r\n\
                echo User Profile: %USERPROFILE%\r\n\
                echo System Root:  %SystemRoot%\r\n\
                echo Temp Folder:  %TEMP%\r\n\
                echo ================================================\r\n\
                mkdir \"%TEMP%\\wsr_build\"\r\n\
                if exist \"%TEMP%\\wsr_build\" echo Created build directory in %TEMP%\\wsr_build\r\n\
                for %%f in (kernel32.dll user32.dll cmd.exe) do echo Verifying VFS entry: %%f\r\n\
                exit /b 0\r\n";
            let _ = fs::write(&sample_bat, bat_content);
        }

        let sample_cmd = desktop.join("deploy_project.cmd");
        if !sample_cmd.exists() {
            let cmd_content = "@echo off\r\n\
                setlocal enabledelayedexpansion\r\n\
                set /a BUILD_NUM=100+42\r\n\
                echo Deploying build #!BUILD_NUM! to C:\\Program Files\\Windows Script Runner...\r\n\
                mkdir \"C:\\Program Files\\Windows Script Runner\\logs\"\r\n\
                echo Build !BUILD_NUM! OK > \"C:\\Program Files\\Windows Script Runner\\logs\\deploy.log\"\r\n\
                type \"C:\\Program Files\\Windows Script Runner\\logs\\deploy.log\"\r\n\
                endlocal\r\n\
                exit /b 0\r\n";
            let _ = fs::write(&sample_cmd, cmd_content);
        }

        let sample_reg = desktop.join("dev_env.reg");
        if !sample_reg.exists() {
            let reg_content = "Windows Registry Editor Version 5.00\r\n\r\n\
                [HKEY_CURRENT_USER\\Environment]\r\n\
                \"WSR_ENV\"=\"production\"\r\n\
                \"ANDROID_WIN_PATH\"=\"C:\\\\Program Files\\\\Windows Script Runner\"\r\n\r\n\
                [HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run]\r\n\
                \"SecurityHealth\"=\"%windir%\\\\system32\\\\SecurityHealthSystray.exe\"\r\n";
            let _ = fs::write(&sample_reg, reg_content);
        }

        // Seed sample batch script in C:\Users\Admin\Downloads so Downloads is not empty by default
        let downloads = self.drive_c.join("Users/Admin/Downloads");
        let dl_readme = downloads.join("setup_notes.txt");
        if !dl_readme.exists() {
            let _ = fs::write(
                &dl_readme,
                "Windows Script Runner VFS Downloads Folder (C:\\Users\\Admin\\Downloads)\r\n\
                 Place .bat, .cmd, .reg, .ps1, .vbs, or PE binaries here or use 'Import File (SAF)' in the toolbar.\r\n",
            );
        }
        let dl_bat = downloads.join("clean_temp.bat");
        if !dl_bat.exists() {
            let _ = fs::write(
                &dl_bat,
                "@echo off\r\necho Cleaning temporary build folder in C:\\Temp...\r\nmkdir \"C:\\Temp\\cache\"\r\nrmdir /s /q \"C:\\Temp\\cache\"\r\necho Clean complete.\r\nexit /b 0\r\n",
            );
        }

        Ok(())
    }

    /// Sanitizes relative path segments so `..` cannot escape the drive root.
    fn sanitize_rel_path(rel: &str) -> PathBuf {
        let mut clean = PathBuf::new();
        for part in rel.split('/').filter(|s| !s.is_empty() && *s != ".") {
            if part == ".." {
                clean.pop();
            } else {
                clean.push(part);
            }
        }
        clean
    }

    /// Translates a Windows path (`C:\Users\Admin`, `D:\Downloads`, `Z:\workspace`) or relative path into a sandboxed Android `PathBuf`.
    pub fn win_to_android_path(&self, raw_path: &str) -> PathBuf {
        let expanded = self.expand_win_env_vars(raw_path.trim().trim_matches('"'));
        let norm = expanded.replace('\\', "/");

        if norm == "~" || norm.starts_with("~/") {
            let home = self.drive_c.join("Users/Admin");
            let rel = norm.strip_prefix('~').unwrap_or("").trim_start_matches('/');
            let target = if rel.is_empty() {
                home
            } else {
                home.join(Self::sanitize_rel_path(rel))
            };
            return Self::resolve_case_insensitive(&target);
        }

        if norm.len() >= 2 && norm.as_bytes()[1] == b':' {
            let drive = (norm.as_bytes()[0] as char).to_ascii_uppercase();
            let rest = norm[2..].trim_start_matches('/');
            let base = match drive {
                'C' => &self.drive_c,
                'D' => &self.drive_d,
                'Z' => &self.drive_z,
                _ => &self.drive_c,
            };
            let joined = if rest.is_empty() {
                base.clone()
            } else {
                base.join(Self::sanitize_rel_path(rest))
            };
            return Self::resolve_case_insensitive(&joined);
        }

        // If already an absolute path inside our VFS or allowed storage, resolve it
        let p = PathBuf::from(&norm);
        if p.is_absolute() {
            if p.starts_with(&self.drive_c) || p.starts_with(&self.drive_d) || p.starts_with(&self.drive_z) || p.starts_with(&self.root_base) {
                return Self::resolve_case_insensitive(&p);
            }
            // Map external absolute paths safely under drive_z
            let rel = norm.trim_start_matches('/');
            return Self::resolve_case_insensitive(&self.drive_z.join(Self::sanitize_rel_path(rel)));
        }

        Self::resolve_case_insensitive(&self.drive_c.join("Users/Admin/Desktop").join(Self::sanitize_rel_path(&norm)))
    }

    /// Converts an Android filesystem path back to a logical Windows path (`C:\...`, `D:\...`, `Z:\...`).
    pub fn android_to_win_path(&self, android_path: &Path) -> String {
        if let Ok(rel) = android_path.strip_prefix(&self.drive_c) {
            let s = rel.to_string_lossy().replace('/', "\\");
            return if s.is_empty() { "C:\\".to_string() } else { format!("C:\\{}", s) };
        }
        if let Ok(rel) = android_path.strip_prefix(&self.drive_d) {
            let s = rel.to_string_lossy().replace('/', "\\");
            return if s.is_empty() { "D:\\".to_string() } else { format!("D:\\{}", s) };
        }
        if let Ok(rel) = android_path.strip_prefix(&self.drive_z) {
            let s = rel.to_string_lossy().replace('/', "\\");
            return if s.is_empty() { "Z:\\".to_string() } else { format!("Z:\\{}", s) };
        }
        let s = android_path.to_string_lossy().replace('/', "\\");
        if s.starts_with('\\') {
            format!("Z:{}", s)
        } else {
            format!("Z:\\{}", s)
        }
    }

    pub fn describe_drive_backing(&self, win_path: &str) -> String {
        let upper = win_path.trim().to_ascii_uppercase();
        if upper.starts_with("D:") {
            format!("D:\\ -> Android Shared/SAF Storage ({})", self.drive_d.display())
        } else if upper.starts_with("Z:") {
            format!("Z:\\ -> App POSIX Sandbox ({})", self.drive_z.display())
        } else {
            format!("C:\\ -> Isolated App-Private Windows VFS ({})", self.drive_c.display())
        }
    }

    pub fn expand_win_env_vars(&self, input: &str) -> String {
        let c_root = self.drive_c.to_string_lossy();
        let replacements = [
            ("%SystemDrive%", "C:".to_string()),
            ("%SYSTEMDRIVE%", "C:".to_string()),
            ("%SystemRoot%", format!("{}/Windows", c_root)),
            ("%SYSTEMROOT%", format!("{}/Windows", c_root)),
            ("%windir%", format!("{}/Windows", c_root)),
            ("%WINDIR%", format!("{}/Windows", c_root)),
            ("%ProgramFiles%", format!("{}/Program Files", c_root)),
            ("%PROGRAMFILES%", format!("{}/Program Files", c_root)),
            ("%ProgramFiles(x86)%", format!("{}/Program Files (x86)", c_root)),
            ("%ProgramData%", format!("{}/ProgramData", c_root)),
            ("%ALLUSERSPROFILE%", format!("{}/ProgramData", c_root)),
            ("%USERPROFILE%", format!("{}/Users/Admin", c_root)),
            ("%HOMEPATH%", format!("{}/Users/Admin", c_root)),
            ("%APPDATA%", format!("{}/Users/Admin/AppData/Roaming", c_root)),
            ("%LOCALAPPDATA%", format!("{}/Users/Admin/AppData/Local", c_root)),
            ("%TEMP%", format!("{}/Users/Admin/AppData/Local/Temp", c_root)),
            ("%TMP%", format!("{}/Users/Admin/AppData/Local/Temp", c_root)),
            ("%PUBLIC%", format!("{}/Users/Public", c_root)),
            ("%COMSPEC%", "/system/bin/sh".to_string()),
        ];
        let mut res = input.to_string();
        for (k, v) in &replacements {
            res = res.replace(k, v);
        }
        res
    }

    pub fn posix_env_preamble(&self) -> String {
        let c = self.drive_c.to_string_lossy();
        let d = self.drive_d.to_string_lossy();
        let z = self.drive_z.to_string_lossy();
        format!(
            "export WSR_DRIVE_C=\"{c}\"\n\
             export WSR_DRIVE_D=\"{d}\"\n\
             export WSR_DRIVE_Z=\"{z}\"\n\
             export SystemDrive=\"C:\"\n\
             export SystemRoot=\"C:\\\\Windows\"\n\
             export windir=\"C:\\\\Windows\"\n\
             export ProgramFiles=\"C:\\\\Program Files\"\n\
             export ProgramData=\"C:\\\\ProgramData\"\n\
             export USERPROFILE=\"C:\\\\Users\\\\Admin\"\n\
             export APPDATA=\"C:\\\\Users\\\\Admin\\\\AppData\\\\Roaming\"\n\
             export LOCALAPPDATA=\"C:\\\\Users\\\\Admin\\\\AppData\\\\Local\"\n\
             export TEMP=\"C:\\\\Users\\\\Admin\\\\AppData\\\\Local\\\\Temp\"\n\
             export TMP=\"C:\\\\Users\\\\Admin\\\\AppData\\\\Local\\\\Temp\"\n"
        )
    }

    pub fn resolve_case_insensitive(target: &Path) -> PathBuf {
        if target.exists() {
            return target.to_path_buf();
        }
        let mut acc = if target.is_absolute() {
            PathBuf::from("/")
        } else {
            PathBuf::new()
        };

        for comp in target.components() {
            if matches!(comp, Component::RootDir | Component::CurDir) {
                continue;
            }
            let comp_str = comp.as_os_str().to_string_lossy();
            let next_candidate = acc.join(comp.as_os_str());
            if next_candidate.exists() {
                acc = next_candidate;
                continue;
            }
            let mut matched = None;
            if let Ok(entries) = fs::read_dir(&acc) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    if name.to_string_lossy().eq_ignore_ascii_case(&comp_str) {
                        matched = Some(entry.path());
                        break;
                    }
                }
            }
            acc = matched.unwrap_or(next_candidate);
        }
        acc
    }

    // ─── VFS FILE CRUD OPERATIONS ───────────────────────────────────────────────
    pub fn create_dir(&self, win_path: &str) -> Result<String, String> {
        let target = self.win_to_android_path(win_path);
        fs::create_dir_all(&target).map_err(|e| format!("Permission denied or failed creating {}: {}", win_path, e))?;
        Ok(self.android_to_win_path(&target))
    }

    pub fn write_text_file(&self, win_path: &str, content: &str) -> Result<String, String> {
        let target = self.win_to_android_path(win_path);
        if let Some(parent) = target.parent() {
            let _ = fs::create_dir_all(parent);
        }
        fs::write(&target, content).map_err(|e| format!("Failed writing {}: {}", win_path, e))?;
        Ok(self.android_to_win_path(&target))
    }

    pub fn rename_item(&self, src_win: &str, dst_win: &str) -> Result<String, String> {
        let src = self.win_to_android_path(src_win);
        if !src.exists() {
            return Err(format!("Source file does not exist: {}", src_win));
        }
        let dst = self.win_to_android_path(dst_win);
        fs::rename(&src, &dst).map_err(|e| format!("Rename failed: {}", e))?;
        Ok(self.android_to_win_path(&dst))
    }

    pub fn copy_item(&self, src_win: &str, dst_win: &str) -> Result<String, String> {
        let src = self.win_to_android_path(src_win);
        if !src.exists() {
            return Err(format!("Source does not exist: {}", src_win));
        }
        let dst = self.win_to_android_path(dst_win);
        if src.is_dir() {
            Self::copy_dir_recursive(&src, &dst).map_err(|e| format!("Directory copy failed: {}", e))?;
        } else {
            if let Some(p) = dst.parent() {
                let _ = fs::create_dir_all(p);
            }
            fs::copy(&src, &dst).map_err(|e| format!("File copy failed: {}", e))?;
        }
        Ok(self.android_to_win_path(&dst))
    }

    fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
        fs::create_dir_all(dst)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let ty = entry.file_type()?;
            if ty.is_dir() {
                Self::copy_dir_recursive(&entry.path(), &dst.join(entry.file_name()))?;
            } else {
                fs::copy(entry.path(), dst.join(entry.file_name()))?;
            }
        }
        Ok(())
    }

    pub fn delete_item(&self, win_path: &str) -> Result<(), String> {
        let target = self.win_to_android_path(win_path);
        if !target.exists() {
            return Err(format!("Item not found: {}", win_path));
        }
        // Prevent deleting root drive directories
        if target == self.drive_c || target == self.drive_d || target == self.drive_z {
            return Err("Cannot delete root VFS drive mount.".to_string());
        }
        if target.is_dir() {
            fs::remove_dir_all(&target).map_err(|e| format!("Delete directory failed: {}", e))
        } else {
            fs::remove_file(&target).map_err(|e| format!("Delete file failed: {}", e))
        }
    }
}
