//! Honest Windows Portable Executable (`.exe`, `.dll`, `.scr`), `.com`, and `.msi` analyzer.
//! Verifies DOS `MZ` header, `PE\0\0` signature, COFF Machine architecture, Optional Header (`PE32` vs `PE32+`),
//! EntryPoint RVA, Subsystem, Section names (`.text`, `.rdata`, `.data`, `.rsrc`), Imported DLLs, and CLR `.NET` metadata.
//! Never fabricates execution when an emulator or runtime is missing.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct PeAnalysis {
    pub status: String, // "VALID_PE", "NON_PE", "DOS_COM", "MSI_PACKAGE", "INVALID_PE"
    pub is_valid_pe: bool,
    pub is_dll: bool,
    pub arch: String,
    pub machine_code: u16,
    pub is_64bit: bool,
    pub subsystem: String,
    pub is_console: bool,
    pub is_dotnet: bool,
    pub entry_point_rva: u32,
    pub image_base: u64,
    pub sections: Vec<String>,
    pub imported_dlls: Vec<String>,
    pub runtime_available: bool,
    pub support_level: String, // "EMULATED", "ANALYSIS_ONLY", "UNSUPPORTED"
    pub support_reason: String,
}

fn has_binary_in_path(bin: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {} >/dev/null 2>&1", bin))
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
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

    // Check OLE Compound Document signature for .msi (D0 CF 11 E0 A1 B1 1A E1)
    if n >= 8 && header_buf[0..8] == [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1] {
        let wine_ok = has_binary_in_path("wine");
        return Ok(PeAnalysis {
            status: "MSI_PACKAGE".to_string(),
            is_valid_pe: false,
            is_dll: false,
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
            runtime_available: wine_ok,
            support_level: if wine_ok { "EMULATED".to_string() } else { "ANALYSIS_ONLY".to_string() },
            support_reason: if wine_ok {
                "Wine msiexec detected on device.".to_string()
            } else {
                "Cannot install MSI: Windows Installer runtime (Wine msiexec) is not installed on this Android device. Package inspection only.".to_string()
            },
        });
    }

    if n < 64 || header_buf[0] != b'M' || header_buf[1] != b'Z' {
        let is_com = ext == "com";
        return Ok(PeAnalysis {
            status: if is_com { "DOS_COM".to_string() } else { "NON_PE".to_string() },
            is_valid_pe: false,
            is_dll: false,
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

    let arch_str = match machine {
        0x8664 => "x86_64 (AMD64)",
        0x014c => "x86 (i386 32-bit)",
        0xAA64 => "ARM64 (Windows on ARM)",
        0x01c0 | 0x01c4 => "ARM32 (Thumb-2)",
        _ => "Unknown COFF Machine",
    };

    let opt_magic = if read_pe >= 26 {
        u16::from_le_bytes([pe_buf[24], pe_buf[25]])
    } else {
        0
    };
    let is_64bit = opt_magic == 0x020B || machine == 0x8664 || machine == 0xAA64;

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

    // Parse section headers (immediately after Optional Header at offset 24 + opt_hdr_size)
    let mut sections = Vec::new();
    let sec_table_start = 24 + opt_hdr_size;
    for i in 0..num_sections.min(16) {
        let off = sec_table_start + i * 40;
        if off + 40 <= read_pe {
            let name_bytes = &pe_buf[off..off + 8];
            let name_end = name_bytes.iter().position(|&b| b == 0).unwrap_or(8);
            let sec_name = String::from_utf8_lossy(&name_bytes[..name_end]).to_string();
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

    // Scan file strings for common imported DLL names (.dll)
    let mut full_bytes = Vec::new();
    let _ = f.seek(SeekFrom::Start(0));
    let _ = f.take(262_144).read_to_end(&mut full_bytes);
    let imported_dlls = scan_imported_dlls(&full_bytes);

    // Check actual runtime availability on this Android device
    if is_dll {
        return Ok(PeAnalysis {
            status: "VALID_PE".to_string(),
            is_valid_pe: true,
            is_dll: true,
            arch: arch_str.to_string(),
            machine_code: machine,
            is_64bit,
            subsystem: subsystem_str.to_string(),
            is_console,
            is_dotnet,
            entry_point_rva,
            image_base,
            sections,
            imported_dlls,
            runtime_available: false,
            support_level: "ANALYSIS_ONLY".to_string(),
            support_reason: "Dynamic Link Library (.dll) is not a standalone executable. PE header, sections, and imports are available for inspection.".to_string(),
        });
    }

    let mono_ok = is_dotnet && has_binary_in_path("mono");
    let box64_wine_ok = is_64bit && has_binary_in_path("box64") && has_binary_in_path("wine64");
    let wine_ok = has_binary_in_path("wine");
    let runtime_available = mono_ok || box64_wine_ok || wine_ok;

    let (support_level, support_reason) = if runtime_available {
        (
            "EMULATED".to_string(),
            format!("Compatible runtime detected for {} PE executable.", arch_str),
        )
    } else {
        (
            "ANALYSIS_ONLY".to_string(),
            format!(
                "Cannot run: {} PE execution backend ({}) is not installed on this Android device. Static PE header analysis is available.",
                arch_str,
                if is_dotnet {
                    "Mono / Wine"
                } else if is_64bit {
                    "Box64 + Wine64"
                } else {
                    "Box86 + Wine"
                }
            ),
        )
    };

    Ok(PeAnalysis {
        status: "VALID_PE".to_string(),
        is_valid_pe: true,
        is_dll: false,
        arch: arch_str.to_string(),
        machine_code: machine,
        is_64bit,
        subsystem: subsystem_str.to_string(),
        is_console,
        is_dotnet,
        entry_point_rva,
        image_base,
        sections,
        imported_dlls,
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
        "PE Header & Binary Inspection\n\
         File:            {}\n\
         Status:          {}\n\
         Valid PE Header: {}\n\
         Architecture:    {} (0x{:04x})\n\
         Address Width:   {}\n\
         Subsystem:       {}\n\
         Entry Point RVA: 0x{:08x} (ImageBase: 0x{:x})\n\
         .NET / CLR:      {}\n\
         PE Sections:     {}\n\
         Imported DLLs:   {}\n\
         Support Level:   {}\n\
         Assessment:      {}",
        path.display(),
        info.status,
        if info.is_valid_pe { "Yes (MZ + PE\\0\\0)" } else { "No" },
        info.arch,
        info.machine_code,
        if info.is_64bit { "64-bit (PE32+)" } else { "32-bit (PE32)" },
        info.subsystem,
        info.entry_point_rva,
        info.image_base,
        if info.is_dotnet { "Yes (Managed CIL)" } else { "No (Native Code)" },
        secs,
        dlls,
        info.support_level,
        info.support_reason
    )
}
