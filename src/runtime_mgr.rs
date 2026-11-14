//! Windows EXE Runtime Manager, Dependency Validator, Installer & Diagnostics (Requirements 3, 4, 7, 10).
//! Probes installed components (`box64`, `wine64` x64 vs `wine` ARM64, `mono`, `Xvfb`, `x11vnc`, `xdotool`, `import`/`scrot`, `proot`),
//! checks Android SELinux / W^X (`execmem`) constraints, builds the Architecture Compatibility Matrix (`x86`, `x64`, `ARM64`, `ARM64EC`),
//! and provides real bootstrap/installation scripts for Termux / `proot-distro` / local runtime bundles.

use crate::vfs::WinVfs;
use std::env;
use std::fs;
use std::net::TcpStream;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ComponentStatus {
    pub id: String,
    pub name: String,
    pub installed: bool,
    pub version_or_path: String,
    pub role: String,
}

#[derive(Debug, Clone)]
pub struct ArchSupportEntry {
    pub arch_code: String,       // "x64", "x86", "ARM64", "ARM64EC"
    pub machine_hex: String,     // "0x8664", "0x014c", "0xAA64", "0xA641"
    pub required_stack: String,
    pub supported_now: bool,
    pub status_label: String,    // "READY (EMULATED)", "READY (NATIVE)", "MISSING RUNTIME", "UNSUPPORTED ON HOST"
    pub notes: String,
}

#[derive(Debug, Clone)]
pub struct RuntimeDiagnostics {
    pub host_os: String,
    pub host_arch: String,
    pub in_apk_sandbox: bool,
    pub box64_installed: bool,
    pub box86_installed: bool,
    pub wine64_x64_installed: bool,
    pub wine_arm64_installed: bool,
    pub wine_wow64_installed: bool,
    pub mono_installed: bool,
    pub xvfb_installed: bool,
    pub xdotool_installed: bool,
    pub x11_capture_installed: bool,
    pub display_server_active: bool,
    pub display_endpoint: String,
    pub can_run_x64_pe: bool,
    pub can_run_x86_pe: bool,
    pub can_run_arm64_pe: bool,
    pub can_run_arm64ec_pe: bool,
    pub components: Vec<ComponentStatus>,
    pub arch_matrix: Vec<ArchSupportEntry>,
    pub blocker_summary: String,
    pub install_commands: Vec<String>,
}

fn check_bin(bin: &str, ver_flag: &str) -> (bool, String) {
    // Also check ~/.wsr_vfs/runtime/bin/<bin>
    let home = env::var("HOME").unwrap_or_else(|_| "/data/user/0/com.vanta.wsr/files".to_string());
    let local_bin = format!("{}/.wsr_vfs/runtime/bin/{}", home, bin);
    if Path::new(&local_bin).exists() {
        if let Ok(out) = Command::new(&local_bin).arg(ver_flag).output() {
            if out.status.success() {
                let first = String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .next()
                    .unwrap_or("installed")
                    .trim()
                    .to_string();
                return (true, format!("{} ({})", first, local_bin));
            }
        }
        return (true, local_bin);
    }

    let sh_cmd = format!("command -v {} 2>/dev/null", bin);
    if let Ok(out) = Command::new("sh").arg("-c").arg(&sh_cmd).output() {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.success() && !p.is_empty() {
            if let Ok(vout) = Command::new(bin).arg(ver_flag).output() {
                let v = String::from_utf8_lossy(&vout.stdout)
                    .lines()
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if !v.is_empty() {
                    return (true, format!("{} ({})", v, p));
                }
            }
            return (true, p);
        }
    }
    (false, "Not installed".to_string())
}

/// Inspects the ELF architecture of the installed `wine` / `wine64` binary so we never confuse
/// an `x86_64` Wine64 build (for Box64) with an `aarch64` native ARM64 Wine build.
fn detect_wine_binary_arch(wine_path_desc: &str) -> (bool, bool) {
    // Returns (is_x86_64_wine, is_aarch64_wine)
    let path_str = wine_path_desc
        .rsplit_once('(')
        .map(|(_, p)| p.trim_end_matches(')'))
        .unwrap_or(wine_path_desc);
    if let Ok(mut f) = fs::File::open(path_str) {
        use std::io::Read;
        let mut elf = [0u8; 20];
        if f.read(&mut elf).unwrap_or(0) >= 20 && &elf[0..4] == b"\x7fELF" {
            let e_machine = u16::from_le_bytes([elf[18], elf[19]]);
            if e_machine == 0x3E {
                return (true, false); // EM_X86_64
            }
            if e_machine == 0xB7 {
                return (false, true); // EM_AARCH64
            }
        }
    }
    // Default assumption if wine64 is present alongside box64: x86_64 wine64
    (true, false)
}

pub fn probe_runtime_environment() -> RuntimeDiagnostics {
    let host_arch = env::consts::ARCH.to_string();
    let host_os = env::consts::OS.to_string();
    let home = env::var("HOME").unwrap_or_default();
    let in_apk_sandbox = home.contains("/data/user/") || home.contains("com.vanta.wsr");

    let (box64_ok, box64_ver) = check_bin("box64", "--version");
    let (box86_ok, box86_ver) = check_bin("box86", "--version");
    let (wine64_ok, wine64_ver) = check_bin("wine64", "--version");
    let (wine_ok, wine_ver) = check_bin("wine", "--version");
    let (mono_ok, mono_ver) = check_bin("mono", "--version");
    let (xvfb_ok, xvfb_ver) = check_bin("Xvfb", "-help");
    let (xdo_ok, xdo_ver) = check_bin("xdotool", "--version");
    let (xwd_ok, xwd_ver) = check_bin("xwd", "-version");
    let (import_ok, import_ver) = check_bin("import", "-version");
    let (proot_ok, proot_ver) = check_bin("proot-distro", "--version");

    let (wine_is_x64, wine_is_arm64) = if wine64_ok {
        detect_wine_binary_arch(&wine64_ver)
    } else if wine_ok {
        detect_wine_binary_arch(&wine_ver)
    } else {
        (false, false)
    };

    let wine64_x64_installed = (wine64_ok || wine_ok) && wine_is_x64;
    let wine_arm64_installed = (wine64_ok || wine_ok) && wine_is_arm64;
    let wine_wow64_installed = wine_ok && box86_ok;
    let x11_capture_installed = xwd_ok || import_ok;

    // Check active display server
    let mut display_server_active = false;
    let mut display_endpoint = "None (Start Xvfb :0 or Termux:X11)".to_string();
    for (port, name) in &[(6000u16, "X11 :0 (TCP 127.0.0.1:6000)"), (5901u16, "VNC/RFB :1 (127.0.0.1:5901)"), (5900u16, "VNC/RFB :0 (127.0.0.1:5900)")] {
        if let Ok(addr) = format!("127.0.0.1:{}", port).parse() {
            if TcpStream::connect_timeout(&addr, Duration::from_millis(80)).is_ok() {
                display_server_active = true;
                display_endpoint = name.to_string();
                break;
            }
        }
    }
    if !display_server_active && Path::new("/tmp/.X11-unix/X0").exists() {
        display_server_active = true;
        display_endpoint = "X11 UNIX Socket (/tmp/.X11-unix/X0)".to_string();
    }

    let can_run_x64_pe = if host_arch == "x86_64" {
        wine64_x64_installed
    } else {
        box64_ok && wine64_x64_installed
    };
    let can_run_x86_pe = wine_wow64_installed;
    let can_run_arm64_pe = (host_arch == "aarch64") && wine_arm64_installed;
    let can_run_arm64ec_pe = false; // Experimental CHPE hybrid ABI

    let components = vec![
        ComponentStatus {
            id: "box64".to_string(),
            name: "Box64 (x86_64-to-ARM64 JIT Translator)".to_string(),
            installed: box64_ok,
            version_or_path: box64_ver,
            role: "Translates 64-bit x64 (0x8664) Linux/Wine64 instructions to ARM64.".to_string(),
        },
        ComponentStatus {
            id: "wine64_x64".to_string(),
            name: "Wine64 (amd64 / x86_64 PE Loader)".to_string(),
            installed: wine64_x64_installed,
            version_or_path: if wine64_x64_installed { wine64_ver.clone() } else { "Not installed".to_string() },
            role: "Loads x64 (0x8664) Windows PE executables and Win32/Win64 DLLs under Box64.".to_string(),
        },
        ComponentStatus {
            id: "wine_arm64".to_string(),
            name: "Wine ARM64 (Native aarch64 PE Loader)".to_string(),
            installed: wine_arm64_installed,
            version_or_path: if wine_arm64_installed { wine_ver.clone() } else { "Not installed".to_string() },
            role: "Required exclusively for native Windows ARM64 (0xAA64) executables.".to_string(),
        },
        ComponentStatus {
            id: "xvfb".to_string(),
            name: "Xvfb / X11 Display Server (:0)".to_string(),
            installed: xvfb_ok || display_server_active,
            version_or_path: if display_server_active { display_endpoint.clone() } else { xvfb_ver },
            role: "Provides virtual X11 framebuffer so Windows GUI windows render and accept input.".to_string(),
        },
        ComponentStatus {
            id: "xdotool".to_string(),
            name: "xdotool (X11 Input & Window Manager Bridge)".to_string(),
            installed: xdo_ok,
            version_or_path: xdo_ver,
            role: "Forwards Android touch, mouse clicks, wheel scroll, and keystrokes to Windows GUI windows.".to_string(),
        },
        ComponentStatus {
            id: "x11_capture".to_string(),
            name: "X11 Framebuffer Capture (import / xwd)".to_string(),
            installed: x11_capture_installed,
            version_or_path: if import_ok { import_ver } else { xwd_ver },
            role: "Streams live X11 window frames into the Android in-app Windows Desktop canvas.".to_string(),
        },
        ComponentStatus {
            id: "mono".to_string(),
            name: "Mono CLR (.NET Executable Runtime)".to_string(),
            installed: mono_ok,
            version_or_path: mono_ver,
            role: "Executes managed .NET CIL (PE32/PE32+) console and WinForms binaries.".to_string(),
        },
        ComponentStatus {
            id: "proot".to_string(),
            name: "proot-distro (Rootfs Container Manager)".to_string(),
            installed: proot_ok,
            version_or_path: proot_ver,
            role: "Provides glibc rootfs required by Box64 + Wine64 on Bionic Android.".to_string(),
        },
    ];

    let arch_matrix = vec![
        ArchSupportEntry {
            arch_code: "x64".to_string(),
            machine_hex: "0x8664".to_string(),
            required_stack: "Box64 + Wine64 (amd64)".to_string(),
            supported_now: can_run_x64_pe,
            status_label: if can_run_x64_pe { "READY (EMULATED)".to_string() } else { "MISSING RUNTIME".to_string() },
            notes: "Primary target for 64-bit Windows apps on ARM64 Android via Box64 + Wine64.".to_string(),
        },
        ArchSupportEntry {
            arch_code: "x86".to_string(),
            machine_hex: "0x014c".to_string(),
            required_stack: "Box86 / Wine WoW64 (i386)".to_string(),
            supported_now: can_run_x86_pe,
            status_label: if can_run_x86_pe { "READY (EMULATED)".to_string() } else { "MISSING RUNTIME".to_string() },
            notes: "32-bit x86 Windows apps require Wine WoW64 (Wine 9.0+) or Box86.".to_string(),
        },
        ArchSupportEntry {
            arch_code: "ARM64".to_string(),
            machine_hex: "0xAA64".to_string(),
            required_stack: "Native ARM64 Wine (aarch64-windows) — NOT Box64".to_string(),
            supported_now: can_run_arm64_pe,
            status_label: if can_run_arm64_pe { "READY (NATIVE)".to_string() } else { "MISSING ARM64 WINE".to_string() },
            notes: "Windows ARM64 binaries cannot run under Box64 + Wine64 (amd64); they require a native ARM64 Wine build.".to_string(),
        },
        ArchSupportEntry {
            arch_code: "ARM64EC".to_string(),
            machine_hex: "0xA641".to_string(),
            required_stack: "Wine ARM64EC CHPE Hybrid Dispatcher".to_string(),
            supported_now: can_run_arm64ec_pe,
            status_label: "EXPERIMENTAL / UNSUPPORTED".to_string(),
            notes: "ARM64EC hybrid binaries interleave ARM64 and x64 thunks and require CHPE Wine.".to_string(),
        },
    ];

    let mut missing = Vec::new();
    if !box64_ok && host_arch == "aarch64" {
        missing.push("box64");
    }
    if !wine64_x64_installed {
        missing.push("wine64 (amd64)");
    }
    if !xvfb_ok && !display_server_active {
        missing.push("Xvfb / X11 display server");
    }

    let blocker_summary = if missing.is_empty() {
        "All required x64 Windows execution and X11 GUI display components are installed.".to_string()
    } else if in_apk_sandbox {
        format!(
            "Missing components: {}. Technical Note: Standalone Android APKs run under Bionic libc and SELinux untrusted_app W^X policies (~500 MB glibc + Box64 + Wine64 + Xvfb rootfs is not bundled inside the lightweight APK). Run the Runtime Installer script in Termux/proot or place a runtime bundle in ~/.wsr_vfs/runtime.",
            missing.join(", ")
        )
    } else {
        format!("Missing runtime packages: {}. Use the Runtime Installer to provision Box64 + Wine64 + Xvfb.", missing.join(", "))
    };

    let install_commands = vec![
        "# Step 1: Install Termux X11, proot-distro, and Box64/Wine64 dependencies".to_string(),
        "pkg update -y && pkg install -y x11-repo tur-repo proot-distro xorg-server-xvfb xdotool imagemagick".to_string(),
        "# Step 2: Install Box64 and Wine64 (amd64) for x64 Windows PE execution on ARM64".to_string(),
        "pkg install -y box64 wine-stable || proot-distro login ubuntu -- bash -c 'apt update && apt install -y wine64 xvfb xdotool imagemagick'".to_string(),
        "# Step 3: Start virtual X11 display server (:0) and initialize Wine prefix".to_string(),
        "Xvfb :0 -screen 0 800x600x24 & export DISPLAY=:0 && box64 wine64 wineboot --init".to_string(),
    ];

    RuntimeDiagnostics {
        host_os,
        host_arch,
        in_apk_sandbox,
        box64_installed: box64_ok,
        box86_installed: box86_ok,
        wine64_x64_installed,
        wine_arm64_installed,
        wine_wow64_installed,
        mono_installed: mono_ok,
        xvfb_installed: xvfb_ok,
        xdotool_installed: xdo_ok,
        x11_capture_installed,
        display_server_active,
        display_endpoint,
        can_run_x64_pe,
        can_run_x86_pe,
        can_run_arm64_pe,
        can_run_arm64ec_pe,
        components,
        arch_matrix,
        blocker_summary,
        install_commands,
    }
}

/// Generates and writes the automated Box64 + Wine64 + Xvfb runtime installer script into the VFS
/// (`C:\Program Files\Windows Script Runner\install_wine_box64.sh`) and attempts non-interactive provisioning
/// if package managers (`pkg` or `apt-get`) are accessible.
pub fn run_runtime_installer() -> (bool, String) {
    let vfs = WinVfs::new();
    let _ = vfs.ensure_initialized();
    let rt_dir = vfs.root_base.join("runtime/bin");
    let _ = fs::create_dir_all(&rt_dir);

    let script_path = vfs
        .drive_c
        .join("Program Files/Windows Script Runner/install_wine_box64.sh");
    let script_body = "#!/bin/sh\n\
        # Windows Script Runner — Box64 + Wine64 + Xvfb Automated Installer\n\
        set -e\n\
        echo '[1/4] Checking environment for package manager (pkg / apt-get)...'\n\
        if command -v pkg >/dev/null 2>&1; then\n\
          pkg update -y\n\
          pkg install -y x11-repo tur-repo proot-distro xorg-server-xvfb xdotool imagemagick box64 wine-stable || true\n\
        elif command -v apt-get >/dev/null 2>&1; then\n\
          apt-get update -y\n\
          apt-get install -y wine64 xvfb xdotool imagemagick || true\n\
        else\n\
          echo '[BLOCKER] No system package manager (pkg / apt-get) is accessible inside the isolated Android APK sandbox.'\n\
          exit 2\n\
        fi\n\
        echo '[2/4] Starting Xvfb virtual X11 server on DISPLAY=:0 (800x600x24)...'\n\
        if command -v Xvfb >/dev/null 2>&1 && ! [ -e /tmp/.X11-unix/X0 ]; then\n\
          nohup Xvfb :0 -screen 0 800x600x24 >/dev/null 2>&1 &\n\
          sleep 1\n\
        fi\n\
        echo '[3/4] Initializing Wine prefix...'\n\
        export DISPLAY=:0\n\
        if command -v box64 >/dev/null 2>&1 && command -v wine64 >/dev/null 2>&1; then\n\
          WINEDEBUG=-all box64 wine64 wineboot --init || true\n\
        elif command -v wine >/dev/null 2>&1; then\n\
          WINEDEBUG=-all wine wineboot --init || true\n\
        fi\n\
        echo '[4/4] Verification complete.'\n";
    let _ = fs::write(&script_path, script_body);

    // Try running the installer if pkg or apt-get is available in PATH
    let out = Command::new("sh")
        .arg(&script_path)
        .output();

    match out {
        Ok(res) => {
            let stdout = String::from_utf8_lossy(&res.stdout).to_string();
            let stderr = String::from_utf8_lossy(&res.stderr).to_string();
            let diag = probe_runtime_environment();
            let ok = diag.can_run_x64_pe || diag.can_run_arm64_pe;
            let combined = format!(
                "{}\n{}\nPost-Install Status: x64 Ready={}, ARM64 Ready={}, X11 Active={}\n{}",
                stdout.trim(),
                stderr.trim(),
                diag.can_run_x64_pe,
                diag.can_run_arm64_pe,
                diag.display_server_active,
                diag.blocker_summary
            );
            (ok, combined.trim().to_string())
        }
        Err(e) => (
            false,
            format!("Installer execution failed: {}. Script saved to {}", e, script_path.display()),
        ),
    }
}
