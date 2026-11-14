//! Core library & Android JNI shared library (`libwsr_jni.so`) for Windows Script Runner (v6.0).

pub mod batch_interp;
pub mod gui_session;
pub mod mcp;
pub mod pe;
pub mod reg;
pub mod runner;
pub mod runtime_mgr;
pub mod server;
pub mod transpiler;
pub mod vfs;

#[cfg(test)]
mod tests {
    use super::pe::inspect_pe_file;
    use super::vfs::WinVfs;
    use std::fs;

    fn build_synthetic_pe(machine: u16, is_64bit: bool, subsystem: u16, sec_name: &[u8; 8]) -> Vec<u8> {
        let mut buf = vec![0u8; 512];
        // DOS MZ Header
        buf[0] = b'M';
        buf[1] = b'Z';
        // e_lfanew at 0x3C = 0x80 (128)
        buf[0x3C] = 0x80;
        let pe = 0x80usize;
        buf[pe..pe + 4].copy_from_slice(b"PE\0\0");
        buf[pe + 4..pe + 6].copy_from_slice(&machine.to_le_bytes());
        buf[pe + 6..pe + 8].copy_from_slice(&1u16.to_le_bytes()); // 1 section
        let opt_size: u16 = if is_64bit { 240 } else { 224 };
        buf[pe + 20..pe + 22].copy_from_slice(&opt_size.to_le_bytes());
        buf[pe + 22..pe + 24].copy_from_slice(&0x0002u16.to_le_bytes()); // Executable

        let opt = pe + 24;
        let magic: u16 = if is_64bit { 0x020B } else { 0x010B };
        buf[opt..opt + 2].copy_from_slice(&magic.to_le_bytes());
        buf[opt + 68..opt + 70].copy_from_slice(&subsystem.to_le_bytes());

        let sec = opt + (opt_size as usize);
        buf[sec..sec + 8].copy_from_slice(sec_name);
        buf
    }

    #[test]
    fn test_separate_x86_x64_arm64_arm64ec_detection_and_arm64_not_box64() {
        let vfs = WinVfs::new();
        let _ = vfs.ensure_initialized();

        // 1. x86 (0x014c)
        let p_x86 = vfs.drive_c.join("Temp/test_x86.exe");
        fs::write(&p_x86, build_synthetic_pe(0x014c, false, 2, b".text\0\0\0")).unwrap();
        let i_x86 = inspect_pe_file(&p_x86).unwrap();
        assert_eq!(i_x86.arch_enum, "x86");
        assert!(i_x86.required_backend.contains("Box86") || i_x86.required_backend.contains("WoW64"));

        // 2. x64 (0x8664)
        let p_x64 = vfs.drive_c.join("Temp/test_x64.exe");
        fs::write(&p_x64, build_synthetic_pe(0x8664, true, 2, b".text\0\0\0")).unwrap();
        let i_x64 = inspect_pe_file(&p_x64).unwrap();
        assert_eq!(i_x64.arch_enum, "x64");
        assert!(i_x64.required_backend.contains("Box64"));

        // 3. ARM64 (0xAA64) — Must NEVER be classified as supported by Box64 + Wine64!
        let p_arm64 = vfs.drive_c.join("Temp/test_arm64.exe");
        fs::write(&p_arm64, build_synthetic_pe(0xAA64, true, 2, b".text\0\0\0")).unwrap();
        let i_arm64 = inspect_pe_file(&p_arm64).unwrap();
        assert_eq!(i_arm64.arch_enum, "ARM64");
        assert!(i_arm64.required_backend.contains("NOT Box64"));
        assert!(i_arm64.support_reason.contains("Box64") && i_arm64.support_reason.contains("NOT"));

        // 4. ARM64EC (0xA641 or .a64xrm section)
        let p_arm64ec = vfs.drive_c.join("Temp/test_arm64ec.exe");
        fs::write(&p_arm64ec, build_synthetic_pe(0x8664, true, 2, b".a64xrm\0")).unwrap();
        let i_arm64ec = inspect_pe_file(&p_arm64ec).unwrap();
        assert_eq!(i_arm64ec.arch_enum, "ARM64EC");
    }
}
