# Windows Script Runner for Android (v5.0 — Real Android SAF Storage, Windows GUI Display Bridge & Native Batch Engine)

Implements all requirements in the **Windows Script Runner PRD (Sections 1–20)**:
- **Real Android Storage Integration (`MainActivity.java`, `src/vfs.rs`, Section 20)**:
  - Connects real device folders (`Downloads`, `Documents`, `Pictures`, `Music`, `Movies`, and custom user folders) via Android Storage Access Framework (`Intent.ACTION_OPEN_DOCUMENT_TREE` and `Intent.ACTION_OPEN_DOCUMENT`) with persisted URI permissions (`takePersistableUriPermission`) and `MediaStore` fallback for Android 11+ scoped storage restrictions.
  - Never populates `C:\Users\Admin\Downloads` with fake files; unconnected Android folders display a clear `Connect Folder (SAF)` / `Browse via MediaStore` / `Pick Single File` flow, and a `REAL ANDROID FOLDER` vs `VIRTUAL VFS` indicator badge is always shown.
  - Stages selected `content://` URIs into `C:\Temp\android_stage` before execution and provides explicit `Stage / Import into Windows VFS` and `Export to Android Folder (SAF)` actions.
- **Windows Application GUI Display Surface & Android Input Bridge (`src/gui_session.rs`, `ui/index.html`, Section 19)**:
  - Distinguishes GUI-subsystem vs Console-subsystem PE binaries from the PE optional header (`src/pe.rs`).
  - Tracks full GUI execution states (`ANALYZING` -> `PREPARING_RUNTIME` -> `STARTING_PROCESS` -> `WAITING_FOR_WINDOW` -> `GUI_VISIBLE` / `CONSOLE_RUNNING` / `EXITED` / `CRASHED` / `UNSUPPORTED`) and probes local X11/RFB display surfaces (`:0` / ports `5900`/`5901`/`6000`).
  - Never shows an empty or fabricated desktop window when a runtime is absent; includes a full Android touch/trackpad pointer and keyboard shortcut bridge (`Left Click`, `Right Click`, `Tab`, `Enter`, `Esc`, `Bksp`, `Up`, `Down`, text input, and viewport zoom).
- **Collapsible Navigation Column & Mobile Responsive Layout (Sections 8 & 9)**:
  - Defaults to a collapsed drawer on phone screens (`<600px`), with 44dp+ touch targets, overflow toolbar (`More`), and resizable 3-state Command Prompt (`collapsed`, `half`, `expanded`).
- **Built-In Native Rust Batch/CMD Interpreter (`src/batch_interp.rs`, Section 6)**:
  - Executes `.bat`/`.cmd` scripts and interactive commands (`echo`, `set /a`, `setlocal enabledelayedexpansion`, `!VAR!`, `if/else`, `for %%v`, `goto`, `call`, `dir`, `copy`, `move`, `del`, `md`, `rd`, `ren`, `type`, `findstr`, `title`) inside the sandboxed VFS.

## Build & GitHub Actions APK Compilation
Push the `windows-script-runner/` directory to GitHub; `.github/workflows/build-apk.yml` automatically runs `cargo test --release`, compiles `libwsr_jni.so` (`arm64-v8a` & `x86_64`), and builds `app-debug.apk` and `app-release.apk`.
