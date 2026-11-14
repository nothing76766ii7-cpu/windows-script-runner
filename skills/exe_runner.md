# exe_runner (Rust PE Inspector + Box64/Wine Bridge)

**Purpose**: Inspect and execute Windows Portable Executables (`.exe`, `.dll`) and Windows Installer packages (`.msi`) on Android.

## Step 1 - Inspect the PE Header with the Rust Engine

```bash
~/.antigem_tools/bin/wsr pe /path/to/program.exe
```

The Rust `pe.rs` module reads the binary's `MZ` and `PE\0\0` headers and reports:
- Target architecture (`x86_64`, `i386`, `ARM64`)
- Address width (`PE32+` 64-bit vs `PE32` 32-bit)
- Subsystem (`Windows Console CUI` vs `Windows GUI`)
- `.NET CLR` runtime presence

## Step 2 - Execute the Binary on Android

- **.NET / Managed CIL Executables**:
  ```bash
  pkg install mono -y
  mono /path/to/program.exe
  ```
- **Native 64-bit (`x86_64`) Windows Executables**:
  ```bash
  WINEDEBUG=-all box64 wine64 /path/to/program.exe
  ```
- **Via Ubuntu `proot-distro` Wine**:
  ```bash
  proot-distro login ubuntu -- env WINEDEBUG=-all wine /path/to/program.exe
  ```
- **Windows Installer (`.msi`) Packages**:
  ```bash
  proot-distro login ubuntu -- wine msiexec /i /path/to/installer.msi
  ```
