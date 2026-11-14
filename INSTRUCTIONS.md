# Windows Script Runner for Android (v4.0 — Real Execution, Compatibility & Mobile UI)

Built according to the **Windows Script Runner PRD v1.0**:
- **Collapsible Mobile Navigation Drawer (`ui/index.html`)**: Defaults to collapsed on narrow screens (`<600px`) with a 44dp+ hamburger toggle, smooth 210ms overlay drawer + scrim, swipe-to-close gesture, Android Back button integration (`window.handleAndroidBack`), and independently collapsible sections (`THIS PC (WINDOWS VFS)`, `QUICK ACCESS`, `FILTER BY TYPE`).
- **Responsive Mobile Layout & Overflow Toolbar**: Primary actions (`Up`, `Refresh`, `Run`, `Analyze`) remain visible on phone screens while secondary actions move cleanly into the `More` menu; file list rows use a compact mobile 2-line layout with explicit execution mode badges (`INTERPRETED`, `NATIVE`, `EMULATED`, `ANALYSIS ONLY`, `UNSUPPORTED`).
- **Native Built-In Rust Batch/CMD Interpreter (`src/batch_interp.rs`)**: Directly executes `.bat` and `.cmd` scripts and interactive DOS commands (`echo`, `set`, `set /a`, `setlocal enabledelayedexpansion`, `!VAR!`, `cd`, `dir`, `cls`, `type`, `copy`, `move`, `del`, `md`, `rd`, `ren`, `if/else`, `for %%v in (...)`, `goto`, `call`, `shift`, `exit /b`, `title`, `where`, `ver`, `help`, `findstr`) inside the isolated Windows VFS (`C:\`, `D:\`, `Z:\`) without leaking raw ANSI/OSC escape sequences.
- **Honest PE Binary Analyzer (`src/pe.rs`) & Separate Analysis Modal**: Verifies `MZ` and `PE\0\0` headers, COFF machine architecture, `PE32`/`PE32+` address width, subsystem, entry point RVA, sections, and imported DLLs in a dedicated modal separate from the Command Prompt. Rejects non-PE files named `.exe` and never fabricates execution when an emulation backend is not installed.
- **Isolated Virtual Registry (`src/reg.rs`)**: Parses, previews, and optionally applies `.reg` files into `~/.wsr_vfs/virtual_registry.conf` after confirmation without touching Android system files.
- **Least-Privilege Android Security & SAF (`AndroidManifest.xml`, `MainActivity.java`)**: Uses app-private VFS storage and Android Storage Access Framework (`Intent.ACTION_OPEN_DOCUMENT`) without `MANAGE_EXTERNAL_STORAGE`.

## Build & GitHub Actions APK Compilation

Push the `windows-script-runner/` directory to GitHub; `.github/workflows/build-apk.yml` automatically runs `cargo test --release`, compiles `libwsr_jni.so` for `arm64-v8a` and `x86_64`, and builds `app-debug.apk` and `app-release.apk`.
