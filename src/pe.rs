//! Honest Windows Portable Executable (`.exe`, `.dll`, `.scr`), `.com`, and `.msi` Analyzer (v6.0).
//! Separately detects `x86` (`0x014c`), `x64` (`0x8664`), `ARM64` (`0xAA64`), and `ARM64EC` (`0xA641` / hybrid CHPE metadata).
//! Strictly enforces that `Box64 + Wine64` ONLY supports `x64` (`AMD64`) Windows binaries and NEVER classifies
//! `ARM64` (`0xAA64`) executables as supported by `Box64 + Wine64`.

use crate::runtime_mgr::probe_runtime_environment;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeArchitecture {
    X86,     // 0x014c (i386 32-bit)
    X64,     // 0x8664 (AMD64 64-bit)
    Arm64,   // 0xAA64 (Pure ARM64 / AArch64)
    Arm64Ec, // 0xA641 or AMD64/ARM64 with CHPE hybrid metadata
    Arm32,   // 0x01c0 / 0x01c4
    Unknown(u16),
}

impl PeArchitecture {
    pub fn code_str(&self) -> &'static str {
        match self {
            PeArchitecture::X86 => "x86",
            PeArchitecture::X64 => "x64",
            PeArchitecture::Arm64 => "ARM64",
            PeArchitecture::Arm64Ec => "ARM64EC",
            PeArchitecture::Arm32 => "ARM32",
            PeArchitecture::Unknown(_) => "UNKNOWN",
        }
    }

    pub fn display_name(&self) -> String {
        match self {
            PeArchitecture::X86 => "x86 (32-bit i386)".to_string(),
            PeArchitecture::X64 => "x64 (64-bit AMD64)".to_string(),
            PeArchitecture::Arm64 => "ARM64 (64-bit AArch64 Native Windows)".to_string(),
            PeArchitecture::Arm64Ec => "ARM64EC (Emulation Compatible Hybrid ABI)".to_string(),
            PeArchitecture::Arm32 => "ARM32 (Thumb-2)".to_string(),
            PeArchitecture::Unknown(c) => format!("Unknown COFF Machine (0x{:04x})", c),
        }
    }

    pub fn required_backend_description(&self, is_dotnet: bool) -> &'static str {
        if is_dotnet {
            return "Mono CLR Runtime or Matching Wine Prefix";
        }
        match self {
            PeArchitecture::X64 => "Box64 (x86_64-to-ARM64 translator) + Wine64 (amd64)",
            PeArchitecture::X86 => "Box86 / Wine WoW64 (32-bit x86 translator) + Wine32",
            PeArchitecture::Arm64 => "Native ARM64 Wine (aarch64-windows PE build) — NOT Box64 + Wine64",
            PeArchitecture::Arm64Ec => "Wine ARM64EC Hybrid Dispatcher + Box64 x64 Emulator",
            PeArchitecture::Arm32 => "Wine ARM32 (Unsupported on 64-bit-only Android 12+ CPUs)",
            PeArchitecture::Unknown(_) => "Unsupported CPU Architecture",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PeAnalysis {
    pub status: String,        // "VALID_PE", "NON_PE", "DOS_COM", "MSI_PACKAGE", "INVALID_PE"
    pub is_valid_pe: bool,
    pub is_dll: bool,
    pub arch_enum: String,     // "x86", "x64", "ARM64", "ARM64EC", "ARM32", "NONE"
    pub arch: String,          // Full descriptive string
    pub machine_code: u16,
    pub is_64bit: bool,
    pub subsystem: String,
    pub is_console: bool,
    pub is_dotnet: bool,
    pub entry_point_rva: u32,
    pub image_base: u64,
    pub sections: Vec<String>,
    pub imported_dlls: Vec<String>,
    pub required_backend: String,
    pub runtime_available: bool,
    pub support_level: String, // "EMULATED", "NATIVE", "ANALYSIS_ONLY", "UNSUPPORTED"
    pub support_reason: String,
}

pub fn inspect_pe_file(path: &Path) -> Result<PeAnalysis, String> {
    let mut f = File::open(path).map_err(|e| format!("Cannot open {}: {}", path.display(), e))?;
    let mut header_buf = [0u8; 4096];
    let n = f.read(&mut header_buf).map_err(|e| e.to_string())?;

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let rt = probe_runtime_environment();

    // Check OLE2 Compound Document signature for .msi
    if n >= 8 && header_buf[0..8] == [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1] {
        let msi_ok = rt.can_run_x64_pe || rt.can_run_arm64_pe;
        return Ok(PeAnalysis {
            status: "MSI_PACKAGE".to_string(),
            is_valid_pe: false,
            is_dll: false,
            arch_enum: "MSI".to_string(),
            arch: "OLE2 Compound Document (Windows Installer .MSI)".to_string(),
            machine_code: 0,
            is_64bit: false,
            subsystem: "Windows Installer (msiexec)".to_string(),
            is_console: false,
            is_dotnet: false,
            entry_point_rva: 0,
            image_base: 0,
            sections: Vec::new(),
            imported_dlls: Vec::new(),
            required_backend: "Wine msiexec.exe".to_string(),
            runtime_available: msi_ok,
            support_level: if msi_ok { "EMULATED".to_string() } else { "ANALYSIS_ONLY".to_string() },
            support_reason: if msi_ok {
                "Wine msiexec runtime detected.".to_string()
            } else {
                format!("Cannot install MSI: Wine runtime is not installed ({})", rt.blocker_summary)
            },
        });
    }

    if n < 64 || header_buf[0] != b'M' || header_buf[1] != b'Z' {
        let is_com = ext == "com";
        return Ok(PeAnalysis {
            status: if is_com { "DOS_COM".to_string() } else { "NON_PE".to_string() },
            is_valid_pe: false,
            is_dll: false,
            arch_enum: "NONE".to_string(),
            arch: if is_com {
                "16-bit MS-DOS .COM Binary".to_string()
            } else {
                "Non-PE File (Missing DOS 'MZ' Signature)".to_string()
            },
            machine_code: 0,
            is_64bit: false,
            subsystem: "None".to_string(),
            is_console: false,
            is_dotnet: false,
            entry_point_rva: 0,
            image_base: 0,
            sections: Vec::new(),
            imported_dlls: Vec::new(),
            required_backend: "None".to_string(),
            runtime_available: false,
            support_level: "UNSUPPORTED".to_string(),
            support_reason: if is_com {
                "Cannot run: 16-bit DOS emulator backend is not installed.".to_string()
            } else {
                "Invalid PE header: this file does not contain a valid Windows 'MZ' / 'PE\\0\\0' header.".to_string()
            },
        });
    }

    let pe_offset = u32::from_le_bytes([
        header_buf[0x3C],
        header_buf[0x3D],
        header_buf[0x3E],
        header_buf[0x3F],
    ]) as usize;

    let mut pe_buf = vec![0u8; 2048];
    f.seek(SeekFrom::Start(pe_offset as u64)).map_err(|e| e.to_string())?;
    let read_pe = f.read(&mut pe_buf).map_err(|e| e.to_string())?;

    if read_pe < 24 || &pe_buf[0..4] != b"PE\0\0" {
        return Ok(PeAnalysis {
            status: "INVALID_PE".to_string(),
            is_valid_pe: false,
            is_dll: false,
            arch_enum: "DOS".to_string(),
            arch: "16-bit MS-DOS MZ Executable (No PE\\0\\0 signature)".to_string(),
            machine_code: 0,
            is_64bit: false,
            subsystem: "MS-DOS".to_string(),
            is_console: true,
            is_dotnet: false,
            entry_point_rva: 0,
            image_base: 0,
            sections: Vec::new(),
            imported_dlls: Vec::new(),
            required_backend: "DOSBox".to_string(),
            runtime_available: false,
            support_level: "UNSUPPORTED".to_string(),
            support_reason: "Cannot run: 16-bit MS-DOS executable requires a DOS emulator.".to_string(),
        });
    }

    let machine = u16::from_le_bytes([pe_buf[4], pe_buf[5]]);
    let num_sections = u16::from_le_bytes([pe_buf[6], pe_buf[7]]) as usize;
    let opt_hdr_size = u16::from_le_bytes([pe_buf[20], pe_buf[21]]) as usize;
    let characteristics = u16::from_le_bytes([pe_buf[22], pe_buf[23]]);
    let is_dll = (characteristics & 0x2000) != 0 || ext == "dll";

    let opt_magic = if read_pe >= 26 {
        u16::from_le_bytes([pe_buf[24], pe_buf[25]])
    } else {
        0
    };
    let is_64bit = opt_magic == 0x020B || machine == 0x8664 || machine == 0xAA64 || machine == 0xA641;

    let entry_point_rva = if read_pe >= 44 {
        u32::from_le_bytes([pe_buf[40], pe_buf[41], pe_buf[42], pe_buf[43]])
    } else {
        0
    };

    let image_base = if is_64bit && read_pe >= 56 {
        u64::from_le_bytes([
            pe_buf[48], pe_buf[49], pe_buf[50], pe_buf[51],
            pe_buf[52], pe_buf[53], pe_buf[54], pe_buf[55],
        ])
    } else if !is_64bit && read_pe >= 56 {
        u32::from_le_bytes([pe_buf[52], pe_buf[53], pe_buf[54], pe_buf[55]]) as u64
    } else {
        0
    };

    let subsystem_code = if read_pe >= 94 {
        u16::from_le_bytes([pe_buf[92], pe_buf[93]])
    } else {
        3
    };
    let (subsystem_str, is_console) = match subsystem_code {
        1 => ("Native Windows Driver", false),
        2 => ("Windows GUI (Win32 GUI)", false),
        3 => ("Windows Console (CUI)", true),
        7 => ("POSIX CUI", true),
        9 => ("Windows CE", false),
        10 | 11 | 12 => ("EFI Binary", false),
        _ => ("Windows Subsystem", false),
    };

    // Parse section headers first so we can also detect ARM64EC hybrid sections (.a64xrm, .hexpthk)
    let mut sections = Vec::new();
    let mut has_arm64ec_section = false;
    let sec_table_start = 24 + opt_hdr_size;
    for i in 0..num_sections.min(16) {
        let off = sec_table_start + i * 40;
        if off + 40 <= read_pe {
            let name_bytes = &pe_buf[off..off + 8];
            let name_end = name_bytes.iter().position(|&b| b == 0).unwrap_or(8);
            let sec_name = String::from_utf8_lossy(&name_bytes[..name_end]).to_string();
            let sec_low = sec_name.to_ascii_lowercase();
            if sec_low == ".a64xrm" || sec_low == ".hexpthk" {
                has_arm64ec_section = true;
            }
            let virt_size = u32::from_le_bytes([
                pe_buf[off + 8],
                pe_buf[off + 9],
                pe_buf[off + 10],
                pe_buf[off + 11],
            ]);
            let virt_addr = u32::from_le_bytes([
                pe_buf[off + 12],
                pe_buf[off + 13],
                pe_buf[off + 14],
                pe_buf[off + 15],
            ]);
            sections.push(format!("{} (RVA: 0x{:06x}, Size: {} B)", sec_name, virt_addr, virt_size));
        }
    }

    // Scan file strings for imported DLLs and ARM64EC markers
    let mut full_bytes = Vec::new();
    let _ = f.seek(SeekFrom::Start(0));
    let _ = f.take(262_144).read_to_end(&mut full_bytes);
    let imported_dlls = scan_imported_dlls(&full_bytes);
    if !has_arm64ec_section {
        if full_bytes.windows(11).any(|w| w.eq_ignore_ascii_case(b"__chpe_meta")) {
            has_arm64ec_section = true;
        }
    }

    let pe_arch = match machine {
        0xA641 => PeArchitecture::Arm64Ec,
        0x8664 if has_arm64ec_section => PeArchitecture::Arm64Ec,
        0xAA64 if has_arm64ec_section => PeArchitecture::Arm64Ec,
        0x8664 => PeArchitecture::X64,
        0x014c => PeArchitecture::X86,
        0xAA64 => PeArchitecture::Arm64,
        0x01c0 | 0x01c4 => PeArchitecture::Arm32,
        other => PeArchitecture::Unknown(other),
    };

    let clr_offset = if opt_magic == 0x020B { 248 } else { 232 };
    let is_dotnet = if read_pe >= clr_offset + 8 {
        let rva = u32::from_le_bytes([
            pe_buf[clr_offset],
            pe_buf[clr_offset + 1],
            pe_buf[clr_offset + 2],
            pe_buf[clr_offset + 3],
        ]);
        let sz = u32::from_le_bytes([
            pe_buf[clr_offset + 4],
            pe_buf[clr_offset + 5],
            pe_buf[clr_offset + 6],
            pe_buf[clr_offset + 7],
        ]);
        rva != 0 && sz != 0
    } else {
        false
    };

    let required_backend = pe_arch.required_backend_description(is_dotnet).to_string();

    if is_dll {
        return Ok(PeAnalysis {
            status: "VALID_PE".to_string(),
            is_valid_pe: true,
            is_dll: true,
            arch_enum: pe_arch.code_str().to_string(),
            arch: pe_arch.display_name(),
            machine_code: machine,
            is_64bit,
            subsystem: subsystem_str.to_string(),
            is_console,
            is_dotnet,
            entry_point_rva,
            image_base,
            sections,
            imported_dlls,
            required_backend,
            runtime_available: false,
            support_level: "ANALYSIS_ONLY".to_string(),
            support_reason: "Dynamic Link Library (.dll) is not a standalone executable. PE header, architecture, sections, and imports are available for inspection.".to_string(),
        });
    }

    // Strict per-architecture runtime compatibility matching (Requirement 2, 3, 4)
    let (runtime_available, support_level, support_reason) = match pe_arch {
        PeArchitecture::X64 => {
            if is_dotnet && rt.mono_installed {
                (true, "EMULATED".to_string(), "Compatible .NET CLR (Mono) runtime installed.".to_string())
            } else if rt.can_run_x64_pe {
                (
                    true,
                    "EMULATED".to_string(),
                    "Compatible x64 execution stack (Box64 + Wine64 amd64) is installed and verified.".to_string(),
                )
            } else {
                (
                    false,
                    "ANALYSIS_ONLY".to_string(),
                    format!(
                        "Cannot execute x64 (AMD64) PE binary: Box64 + Wine64 (amd64) runtime is not installed. ({})",
                        rt.blocker_summary
                    ),
                )
            }
        }
        PeArchitecture::X86 => {
            if is_dotnet && rt.mono_installed {
                (true, "EMULATED".to_string(), "Compatible .NET CLR (Mono) runtime installed.".to_string())
            } else if rt.can_run_x86_pe {
                (
                    true,
                    "EMULATED".to_string(),
                    "Compatible 32-bit x86 execution stack (Box86 / Wine WoW64) is installed.".to_string(),
                )
            } else {
                (
                    false,
                    "ANALYSIS_ONLY".to_string(),
                    "Cannot execute 32-bit x86 (i386) PE binary: requires Wine WoW64 or Box86 + 32-bit libraries, which are not installed on this ARM64 Android device.".to_string(),
                )
            }
        }
        PeArchitecture::Arm64 => {
            // CRITICAL REQUIREMENT 4: NEVER classify Windows ARM64 (0xAA64) as supported by Box64 + Wine64!
            if rt.can_run_arm64_pe {
                (
                    true,
                    "NATIVE".to_string(),
                    "Native ARM64 Wine (aarch64-windows PE build) is installed and verified.".to_string(),
                )
            } else if rt.box64_installed && rt.wine64_x64_installed {
                (
                    false,
                    "UNSUPPORTED".to_string(),
                    "Architecture Mismatch: This is a Windows ARM64 (0xAA64) executable. Box64 + Wine64 (amd64) only translates x64 (0x8664) binaries and CANNOT run Windows ARM64 executables. A native ARM64 Wine build (wine-arm64) is required.".to_string(),
                )
            } else {
                (
                    false,
                    "UNSUPPORTED".to_string(),
                    "Cannot execute Windows ARM64 (0xAA64) binary: requires a native ARM64 Wine build (wine-arm64). Note: Box64 + Wine64 (x64) does NOT support Windows ARM64 binaries.".to_string(),
                )
            }
        }
        PeArchitecture::Arm64Ec => {
            if rt.can_run_arm64ec_pe {
                (
                    true,
                    "EMULATED".to_string(),
                    "ARM64EC hybrid Wine runtime is installed.".to_string(),
                )
            } else {
                (
                    false,
                    "UNSUPPORTED".to_string(),
                    "Cannot execute Windows ARM64EC hybrid binary: requires Wine with ARM64EC CHPE thunking support, which is not installed on this device.".to_string(),
                )
            }
        }
        PeArchitecture::Arm32 => (
            false,
            "UNSUPPORTED".to_string(),
            "Cannot execute Windows ARM32 (Thumb-2) binary: modern ARM64 Android kernels do not support 32-bit Windows ARMv7 execution.".to_string(),
        ),
        PeArchitecture::Unknown(code) => (
            false,
            "UNSUPPORTED".to_string(),
            format!("Unsupported COFF machine type 0x{:04x}.", code),
        ),
    };

    Ok(PeAnalysis {
        status: "VALID_PE".to_string(),
        is_valid_pe: true,
        is_dll: false,
        arch_enum: pe_arch.code_str().to_string(),
        arch: pe_arch.display_name(),
        machine_code: machine,
        is_64bit,
        subsystem: subsystem_str.to_string(),
        is_console,
        is_dotnet,
        entry_point_rva,
        image_base,
        sections,
        imported_dlls,
        required_backend,
        runtime_available,
        support_level,
        support_reason,
    })
}

fn scan_imported_dlls(bytes: &[u8]) -> Vec<String> {
    let mut found = Vec::new();
    let mut cur = Vec::new();
    for &b in bytes {
        if (0x20..=0x7E).contains(&b) {
            cur.push(b);
            if cur.len() > 64 {
                cur.clear();
            }
        } else {
            if cur.len() >= 5 && cur.len() <= 40 {
                if let Ok(s) = std::str::from_utf8(&cur) {
                    let low = s.to_ascii_lowercase();
                    if low.ends_with(".dll")
                        && s.chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
                    {
                        if !found.iter().any(|existing: &String| existing.eq_ignore_ascii_case(s)) {
                            found.push(s.to_string());
                            if found.len() >= 16 {
                                break;
                            }
                        }
                    }
                }
            }
            cur.clear();
        }
    }
    found
}

pub fn format_pe_summary(path: &Path, info: &PeAnalysis) -> String {
    let secs = if info.sections.is_empty() {
        "None".to_string()
    } else {
        info.sections.join(", ")
    };
    let dlls = if info.imported_dlls.is_empty() {
        "None detected".to_string()
    } else {
        info.imported_dlls.join(", ")
    };
    format!(
        "PE Header & Architecture Compatibility Inspection\n\
         File:             {}\n\
         Status:           {}\n\
         Valid PE Header:  {}\n\
         Detected Arch:    {} [{}] (Machine: 0x{:04x})\n\
         Address Width:    {}\n\
         Subsystem:        {}\n\
         Entry Point RVA:  0x{:08x} (ImageBase: 0x{:x})\n\
         .NET / CLR:       {}\n\
         Required Backend: {}\n\
         PE Sections:      {}\n\
         Imported DLLs:    {}\n\
         Support Level:    {}\n\
         Diagnostic:       {}",
        path.display(),
        info.status,
        if info.is_valid_pe { "Yes (MZ + PE\\0\\0)" } else { "No" },
        info.arch,
        info.arch_enum,
        info.machine_code,
        if info.is_64bit { "64-bit (PE32+)" } else { "32-bit (PE32)" },
        info.subsystem,
        info.entry_point_rva,
        info.image_base,
        if info.is_dotnet { "Yes (Managed CIL)" } else { "No (Native Code)" },
        info.required_backend,
        secs,
        dlls,
        info.support_level,
        info.support_reason
    )
}
