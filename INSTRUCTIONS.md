# Windows Script Runner (Rust VFS + PE EXE + Android APK Edition v3.0)

Execute `.bat`, `.cmd`, `.exe`, `.msi`, `.ps1`, `.vbs`, `.reg`, and `.rs` scripts on Android with a real **Windows Virtual File System (`C:\`, `D:\`, `Z:\`)**, a native **Rust PE executable header inspector & Box64/Wine launcher**, and an embedded **Windows Explorer UI** using inline SVG vector graphics (zero emojis).

## How to Push to GitHub and Auto-Compile the APK

1. Extract `windows-script-runner.zip` and initialize a Git repository inside the `windows-script-runner/` folder:
   ```bash
   cd windows-script-runner
   git init
   git add .
   git commit -m "Initial commit: Rust Windows VFS + PE EXE Runner + Android APK"
   git branch -M main
   git remote add origin https://github.com/<your-username>/<your-repo>.git
   git push -u origin main
   ```
2. Open the **Actions** tab on your GitHub repository.
3. The workflow `.github/workflows/build-apk.yml` automatically:
   - Installs JDK 17, Android SDK/NDK r26b, Rust stable (`aarch64-linux-android` & `x86_64-linux-android`), and `cargo-ndk`.
   - Runs `cargo test --release` across the Rust VFS, `.bat`/`.cmd` transpiler, `.reg` parser, and PE `.exe` analyzer.
   - Compiles `libwsr_jni.so` for both `arm64-v8a` and `x86_64`.
   - Packages the standalone Android APK (`app-debug.apk` and `app-release.apk`) and uploads it as the `windows-script-runner-apk` downloadable artifact.

## Key Capabilities

- **Windows Virtual File System (`src/vfs.rs`)**:
  - Mounts `C:\` to `~/.wsr_vfs/drive_c` with real `C:\Windows\System32`, `C:\Program Files`, `C:\ProgramData`, `C:\Users\Admin\Desktop`, and `C:\Temp` directories.
  - Mounts `D:\` to Android shared storage (`/sdcard` / `/storage/emulated/0`) and `Z:\` to the Android/Linux root filesystem (`/`).
  - Resolves Windows paths case-insensitively on Linux/Android and exports standard Windows environment variables (`%USERPROFILE%`, `%SystemRoot%`, `%APPDATA%`, `%TEMP%`, `%ProgramFiles%`).
- **Batch & Command Transpiler (`src/transpiler.rs`, `tools/bat2sh.rs`)**:
  - Translates `.bat` and `.cmd` scripts into POSIX `.sh` while mapping `C:\`, `D:\`, `Z:\`, `%~dp0`, `set /a`, `set /p`, `if exist`, `for %%f in (...)`, `call`, `dir`, `ver`, `ipconfig`, `tasklist`, `findstr`, and `where`.
- **PE `.exe` / `.dll` / `.msi` Support (`src/pe.rs`, `skills/exe_runner.md`)**:
  - Parses DOS (`MZ`) and PE (`PE\0\0`) binary headers in pure Rust to detect CPU architecture (`x86_64`, `i386`, `ARM64`), subsystem (`Win32 GUI` vs `Windows Console CUI`), and `.NET CLR` metadata.
  - Launches `.exe` and `.msi` files automatically through `mono` (for `.NET` binaries), `box64 wine64` (for 64-bit PE32+ binaries), or `wine` (inside Termux / `proot-distro` Ubuntu).
