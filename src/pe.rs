//! Native Rust Windows Portable Executable (`.exe`, `.dll`) and `.msi` header inspector and execution bridge.
//! Parses DOS (`MZ`) and PE (`PE\0\0`) headers to determine target CPU architecture, subsystem (Console vs GUI),
//! 32-bit vs 64-bit (`PE32` vs `PE32+`), and CLR/.NET presence, then builds the optimal execution command for Android
//! (Box64 + Wine64, Box86 + Wine, Mono, or proot-distro Ubuntu Wine).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct PeInfo {
    pub is_valid_pe: bool,
    pub arch: String,
    pub machine_code: u16,
    pub is_64bit: bool,
    pub subsystem: String,
    pub is_console: bool,
    pub is_dotnet: bool,
    pub recommended_runner: String,
}

pub fn inspect_pe_file(path: &Path) -> Result<PeInfo, String> {
    let mut f = File::open(path).map_err(|e| format!("Cannot open {}: {}", path.display(), e))?;
    let mut dos_hdr = [0u8; 64];
    let n = f.read(&mut dos_hdr).map_err(|e| e.to_string())?;
    if n < 64 || dos_hdr[0] != b'M' || dos_hdr[1] != b'Z' {
        return Ok(PeInfo {
            is_valid_pe: false,
            arch: "Non-PE / Legacy DOS or Script".to_string(),
            machine_code: 0,
            is_64bit: false,
            subsystem: "Unknown".to_string(),
            is_console: true,
            is_dotnet: false,
            recommended_runner: "wine".to_string(),
        });
    }

    let pe_offset = u32::from_le_bytes([dos_hdr[0x3C], dos_hdr[0x3D], dos_hdr[0x3E], dos_hdr[0x3F]]) as u64;
    f.seek(SeekFrom::Start(pe_offset)).map_err(|e| e.to_string())?;

    let mut pe_hdr = [0u8; 264];
    let read_bytes = f.read(&mut pe_hdr).map_err(|e| e.to_string())?;
    if read_bytes < 24 || &pe_hdr[0..4] != b"PE\0\0" {
        return Ok(PeInfo {
            is_valid_pe: false,
            arch: "16-bit MS-DOS MZ Executable".to_string(),
            machine_code: 0,
            is_64bit: false,
            subsystem: "DOS".to_string(),
            is_console: true,
            is_dotnet: false,
            recommended_runner: "dosbox / winevdm".to_string(),
        });
    }

    let machine = u16::from_le_bytes([pe_hdr[4], pe_hdr[5]]);
    let arch_str = match machine {
        0x8664 => "x86_64 (AMD64)",
        0x014c => "x86 (i386 32-bit)",
        0xAA64 => "ARM64 (Windows on ARM)",
        0x01c0 | 0x01c4 => "ARM32",
        _ => "Unknown PE Machine",
    };

    let opt_magic = if read_bytes >= 26 {
        u16::from_le_bytes([pe_hdr[24], pe_hdr[25]])
    } else {
        0
    };
    let is_64bit = opt_magic == 0x020B || machine == 0x8664 || machine == 0xAA64;

    // Subsystem offset in Optional Header is 68 bytes from start of Optional Header (24 + 68 = 92)
    let subsystem_code = if read_bytes >= 94 {
        u16::from_le_bytes([pe_hdr[92], pe_hdr[93]])
    } else {
        3
    };
    let (subsystem_str, is_console) = match subsystem_code {
        1 => ("Native Driver", true),
        2 => ("Windows GUI (Win32 GUI)", false),
        3 => ("Windows Console (CUI)", true),
        7 => ("POSIX CUI", true),
        _ => ("Windows Application", false),
    };

    // Check CLR Runtime Header data directory (DataDirectory[14]) for .NET assembly
    // For PE32 (0x10B), DataDirectory starts at offset 24 + 96 = 120; entry 14 is at 120 + 14*8 = 232
    // For PE32+ (0x20B), DataDirectory starts at offset 24 + 112 = 136; entry 14 is at 136 + 14*8 = 248
    let clr_offset = if opt_magic == 0x020B { 248 } else { 232 };
    let is_dotnet = if read_bytes >= clr_offset + 8 {
        let rva = u32::from_le_bytes([
            pe_hdr[clr_offset],
            pe_hdr[clr_offset + 1],
            pe_hdr[clr_offset + 2],
            pe_hdr[clr_offset + 3],
        ]);
        let size = u32::from_le_bytes([
            pe_hdr[clr_offset + 4],
            pe_hdr[clr_offset + 5],
            pe_hdr[clr_offset + 6],
            pe_hdr[clr_offset + 7],
        ]);
        rva != 0 && size != 0
    } else {
        false
    };

    let runner = if is_dotnet {
        "mono / box64 wine64".to_string()
    } else if machine == 0xAA64 {
        "wine64 (native ARM64)".to_string()
    } else if is_64bit {
        "box64 wine64".to_string()
    } else {
        "box86 wine".to_string()
    };

    Ok(PeInfo {
        is_valid_pe: true,
        arch: arch_str.to_string(),
        machine_code: machine,
        is_64bit,
        subsystem: subsystem_str.to_string(),
        is_console,
        is_dotnet,
        recommended_runner: runner,
    })
}

pub fn format_pe_summary(path: &Path, info: &PeInfo) -> String {
    format!(
        "PE Binary Analysis for {}:\n\
         - Valid PE Header: {}\n\
         - Architecture:    {} (0x{:04x})\n\
         - Address Width:   {}\n\
         - Subsystem:       {}\n\
         - .NET / CLR:      {}\n\
         - Android Runner:  {}",
        path.display(),
        if info.is_valid_pe { "Yes (MZ + PE\\0\\0)" } else { "No" },
        info.arch,
        info.machine_code,
        if info.is_64bit { "64-bit (PE32+)" } else { "32-bit (PE32)" },
        info.subsystem,
        if info.is_dotnet { "Yes (Managed CIL)" } else { "No (Native Win32/Win64)" },
        info.recommended_runner
    )
}

/// Builds a multi-tier Android execution command for `.exe` and `.msi` files.
/// Tier 1: Inspects PE headers and prints architecture & subsystem diagnostics.
/// Tier 2: If `.NET` and `mono` is installed, runs directly with `mono`.
/// Tier 3: If `box64`/`wine` is installed in Termux or `proot-distro` Ubuntu, executes via Wine/Box64.
pub fn build_exe_command(file_path: &str, args: &str, is_msi: bool) -> String {
    let a = if args.trim().is_empty() {
        String::new()
    } else {
        format!(" {}", args.trim())
    };

    if is_msi {
        return format!(
            "if command -v wine >/dev/null 2>&1; then \
               wine msiexec /i \"{f}\"{a}; \
             elif command -v proot-distro >/dev/null 2>&1; then \
               proot-distro login ubuntu -- wine msiexec /i \"{f}\"{a}; \
             else \
               echo \"[MSI Runner] Wine is required to install {f}. Run Setup Rust / Wine to configure Box64 + Wine.\"; \
             fi",
            f = file_path,
            a = a
        );
    }

    let pe_check = inspect_pe_file(Path::new(file_path)).ok();
    let header_msg = if let Some(ref info) = pe_check {
        format!(
            "[PE Inspector] Arch: {} | Subsystem: {} | .NET: {} | Runner: {}",
            info.arch,
            info.subsystem,
            if info.is_dotnet { "Yes" } else { "No" },
            info.recommended_runner
        )
    } else {
        "[PE Inspector] Analyzing Windows executable...".to_string()
    };

    format!(
        "echo \"{msg}\" && \
         if command -v mono >/dev/null 2>&1 && file \"{f}\" 2>/dev/null | grep -qi 'Mono\\|.Net'; then \
           mono \"{f}\"{a}; \
         elif command -v box64 >/dev/null 2>&1 && command -v wine64 >/dev/null 2>&1; then \
           WINEDEBUG=-all box64 wine64 \"{f}\"{a}; \
         elif command -v wine >/dev/null 2>&1; then \
           WINEDEBUG=-all wine \"{f}\"{a}; \
         elif command -v proot-distro >/dev/null 2>&1; then \
           proot-distro login ubuntu -- env WINEDEBUG=-all wine \"{f}\"{a}; \
         else \
           echo \"[EXE Compatibility Layer] Native PE header verified. To execute Win32/Win64 binaries on Android ARM64, install wine/box64 via Toolbar -> Setup Rust.\"; \
         fi",
        msg = header_msg,
        f = file_path,
        a = a
    )
}
