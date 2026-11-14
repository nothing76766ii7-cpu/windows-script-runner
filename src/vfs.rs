//! Windows Virtual File System (`C:\`, `D:\`, `Z:\`) and path translator for Android / Linux.
//! Provides real directory tree provisioning (`C:\Windows\System32`, `C:\Program Files`, `C:\Users\Admin`, `C:\Temp`),
//! drive letter translation (`C:\`, `D:\`, `Z:\`), case-insensitive path resolution, and Windows environment variables.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

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
                let home = env::var("HOME").unwrap_or_else(|_| "/data/data/com.vanta.wsr/files/home".to_string());
                PathBuf::from(home).join(".wsr_vfs")
            });
        let drive_c = base.join("drive_c");
        let drive_d = if Path::new("/sdcard").exists() {
            PathBuf::from("/sdcard")
        } else if Path::new("/storage/emulated/0").exists() {
            PathBuf::from("/storage/emulated/0")
        } else {
            base.join("drive_d")
        };
        let drive_z = PathBuf::from("/");

        Self {
            root_base: base,
            drive_c,
            drive_d,
            drive_z,
        }
    }

    /// Ensures the full Windows directory hierarchy exists inside `drive_c` along with realistic default batch/reg files.
    pub fn ensure_initialized(&self) -> Result<(), String> {
        let dirs = [
            self.drive_c.join("Windows/System32"),
            self.drive_c.join("Windows/SysWOW64"),
            self.drive_c.join("Program Files/Windows Script Runner"),
            self.drive_c.join("Program Files (x86)"),
            self.drive_c.join("ProgramData/Microsoft/Windows/Start Menu/Programs/Startup"),
            self.drive_c.join("Users/Admin/Desktop"),
            self.drive_c.join("Users/Admin/Documents"),
            self.drive_c.join("Users/Admin/Downloads"),
            self.drive_c.join("Users/Admin/AppData/Local/Temp"),
            self.drive_c.join("Users/Admin/AppData/Roaming"),
            self.drive_c.join("Users/Public"),
            self.drive_c.join("Temp"),
            self.root_base.join("drive_d"),
        ];

        for d in &dirs {
            fs::create_dir_all(d).map_err(|e| format!("Failed creating {}: {}", d.display(), e))?;
        }

        // Seed starter scripts in C:\Users\Admin\Desktop if empty
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
                for %%f in (kernel32.dll user32.dll cmd.exe) do echo Checking System32 component: %%f\r\n\
                exit /b 0\r\n";
            let _ = fs::write(&sample_bat, bat_content);
        }

        let sample_cmd = desktop.join("deploy_project.cmd");
        if !sample_cmd.exists() {
            let cmd_content = "@echo off\r\n\
                set /a BUILD_NUM=100+42\r\n\
                echo Deploying build #%BUILD_NUM% to C:\\Program Files\\Windows Script Runner...\r\n\
                mkdir \"C:\\Program Files\\Windows Script Runner\\logs\"\r\n\
                echo Build %BUILD_NUM% OK > \"C:\\Program Files\\Windows Script Runner\\logs\\deploy.log\"\r\n\
                type \"C:\\Program Files\\Windows Script Runner\\logs\\deploy.log\"\r\n\
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

        // Seed System32 POSIX wrappers for common Win32 CLI utilities so scripts calling them directly can run
        let sys32 = self.drive_c.join("Windows/System32");
        let cmd_wrapper = sys32.join("ipconfig");
        if !cmd_wrapper.exists() {
            let _ = fs::write(&cmd_wrapper, "#!/bin/sh\nip addr show 2>/dev/null || ifconfig 2>/dev/null || echo 'Active Network Interface: wlan0 (Android VFS)'\n");
        }
        let tasklist_wrapper = sys32.join("tasklist");
        if !tasklist_wrapper.exists() {
            let _ = fs::write(&tasklist_wrapper, "#!/bin/sh\nps -ef 2>/dev/null || ps\n");
        }
        let systeminfo_wrapper = sys32.join("systeminfo");
        if !systeminfo_wrapper.exists() {
            let _ = fs::write(&systeminfo_wrapper, "#!/bin/sh\necho 'OS Name:                   Microsoft Windows 11 Pro (Android WSR VFS Layer)'\necho \"System Kernel:             $(uname -sr)\"\necho \"Architecture:              $(uname -m)\"\n");
        }
        let where_wrapper = sys32.join("where");
        if !where_wrapper.exists() {
            let _ = fs::write(&where_wrapper, "#!/bin/sh\nwhich \"$@\"\n");
        }

        Ok(())
    }

    /// Translates a Windows path (`C:\Users\Admin`, `D:\Downloads`, `Z:\sdcard`, or `%USERPROFILE%\Desktop`)
    /// into a real Android/Linux filesystem `PathBuf`, performing case-insensitive segment matching when possible.
    pub fn win_to_android_path(&self, raw_path: &str) -> PathBuf {
        let expanded = self.expand_win_env_vars(raw_path.trim().trim_matches('"'));
        let norm = expanded.replace('\\', "/");

        if norm == "~" || norm.starts_with("~/") {
            let home = self.drive_c.join("Users/Admin");
            let rel = norm.strip_prefix('~').unwrap_or("").trim_start_matches('/');
            let target = if rel.is_empty() { home } else { home.join(rel) };
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
                base.join(rest)
            };
            return Self::resolve_case_insensitive(&joined);
        }

        Self::resolve_case_insensitive(&PathBuf::from(norm))
    }

    /// Converts a real Android/Linux path back to a canonical Windows Drive path (`C:\...`, `D:\...`, or `Z:\...`).
    pub fn android_to_win_path(&self, android_path: &Path) -> String {
        if let Ok(rel) = android_path.strip_prefix(&self.drive_c) {
            let s = rel.to_string_lossy().replace('/', "\\");
            return if s.is_empty() { "C:\\".to_string() } else { format!("C:\\{}", s) };
        }
        if let Ok(rel) = android_path.strip_prefix(&self.drive_d) {
            let s = rel.to_string_lossy().replace('/', "\\");
            return if s.is_empty() { "D:\\".to_string() } else { format!("D:\\{}", s) };
        }
        let s = android_path.to_string_lossy().replace('/', "\\");
        if s.starts_with('\\') {
            format!("Z:{}", s)
        } else {
            format!("Z:\\{}", s)
        }
    }

    /// Expands Windows `%VAR%` strings using the VFS Windows environment mapping.
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
            ("%COMSPEC%", "/bin/sh".to_string()),
        ];
        let mut res = input.to_string();
        for (k, v) in &replacements {
            res = res.replace(k, v);
        }
        res
    }

    /// Generates POSIX shell preamble that exports all Windows environment variables and drive mounts.
    pub fn posix_env_preamble(&self) -> String {
        let c = self.drive_c.to_string_lossy();
        let d = self.drive_d.to_string_lossy();
        format!(
            "export WSR_DRIVE_C=\"{c}\"\n\
             export WSR_DRIVE_D=\"{d}\"\n\
             export WSR_DRIVE_Z=\"/\"\n\
             export SystemDrive=\"C:\"\n\
             export SystemRoot=\"{c}/Windows\"\n\
             export windir=\"{c}/Windows\"\n\
             export ProgramFiles=\"{c}/Program Files\"\n\
             export ProgramData=\"{c}/ProgramData\"\n\
             export USERPROFILE=\"{c}/Users/Admin\"\n\
             export APPDATA=\"{c}/Users/Admin/AppData/Roaming\"\n\
             export LOCALAPPDATA=\"{c}/Users/Admin/AppData/Local\"\n\
             export TEMP=\"{c}/Users/Admin/AppData/Local/Temp\"\n\
             export TMP=\"{c}/Users/Admin/AppData/Local/Temp\"\n\
             export PATH=\"{c}/Windows/System32:$HOME/.antigem_tools/bin:$PATH\"\n"
        )
    }

    /// Resolves each path component case-insensitively on a case-sensitive Linux/Android filesystem.
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
            let comp_str = comp.as_os_str().to_string_lossy();
            if comp_str == "/" || comp_str.is_empty() {
                continue;
            }
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
}
