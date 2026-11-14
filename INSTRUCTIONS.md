# Windows Script Runner for Android (v6.0 — Critical EXE Runtime, Architecture Matrix & X11 Display Bridge)

Implements all requirements in **Windows Script Runner — Critical EXE Runtime Fix**:
1. **Separate `x86`, `x64`, `ARM64`, and `ARM64EC` PE Classification (`src/pe.rs`)**:
   - Inspects COFF machine codes (`0x014c` for `x86`, `0x8664` for `x64`, `0xAA64` for `ARM64`, `0xA641` and `.a64xrm`/`.hexpthk`/`__chpe_meta` sections for `ARM64EC`) and optional header magic (`PE32` vs `PE32+`).
2. **Strict Architecture-to-Runtime Matching (Never Classifies `ARM64` as `Box64 + Wine64`)**:
   - `x64` (`0x8664`) -> `Box64 (x86_64-to-ARM64 translator) + Wine64 (amd64)`
   - `x86` (`0x014c`) -> `Box86 / Wine WoW64 (32-bit i386)`
   - `ARM64` (`0xAA64`) -> `Native ARM64 Wine (aarch64-windows)` — explicitly rejects `Box64 + Wine64` with a clear architecture-mismatch diagnostic.
   - `ARM64EC` (`0xA641`) -> `Wine ARM64EC CHPE Hybrid Dispatcher`
3. **Runtime Installation, Validation & Diagnostics Page (`src/runtime_mgr.rs`, `ui/index.html`)**:
   - Inspects the ELF headers (`e_machine`) of installed `wine`/`wine64` binaries to distinguish an `x86_64` Wine64 build from an `aarch64` ARM64 Wine build.
   - Generates `C:\Program Files\Windows Script Runner\install_wine_box64.sh` and provides a dedicated **EXE Runtime Diagnostics & Architecture Matrix** modal showing installed components, per-architecture readiness, and technical blockers.
4. **Live X11 Graphical Display Surface & Window/Input Bridge (`src/gui_session.rs`, `ui/index.html`)**:
   - Starts `Xvfb :0` when installed, enumerates visible X11 windows via `xdotool`, streams live PNG frames (`import -window root`) to the HTML5 Canvas, and forwards Android touch, trackpad pointer, mouse wheel scroll, keyboard keys, and window operations (`Min Win`, `Max Win`, `Close Win`, window focus switcher).
   - Never displays a fake Windows GUI when no real process or X11 window is active.
