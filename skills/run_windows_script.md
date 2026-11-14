# run_windows_script (Rust Edition)

**Purpose**: Execute or interpret Windows-only and Rust script files on Android via the AntiGem terminal and the native Rust tool suite (`wsr`, `bat2sh`, `reg_parser`).

## Supported types

| Extension       | Strategy                                  | Sub-skill         |
|-----------------|-------------------------------------------|-------------------|
| `.bat` / `.cmd` | Transpile via Rust `bat2sh` -> `.sh`, run | `bat_transpiler`  |
| `.ps1`          | PowerShell Core ARM64                     | `ps1_android`     |
| `.vbs`          | Wine + `cscript.exe //nologo`             | `vbs_wine`        |
| `.reg`          | Parse + report + apply via Rust `reg_parser` | `reg_handler`  |
| `.rs`           | Compile via `rustc -O` and execute        | -                 |
| `.py`           | `python3 <file>` direct                   | -                 |
| `.js` / `.mjs`  | `node <file>` direct                      | -                 |
| `.sh`           | `chmod +x && sh <file>` direct            | -                 |
| `.exe` / `.msi` | Cannot run - Windows PE binary            | Report to user    |

## Workflow

1. User provides a file path or pastes script content.
2. Detect extension (`wsr run <path>` or `detectStrategy`).
3. Follow the matching sub-skill.
4. Run the resulting command via the terminal MCP tool.
5. Stream and report output.
6. After execution, list any skipped Windows-only lines.

## MCP Tools

- `wsr_list_directory`    - list files in a path
- `wsr_run_script`        - build + run command for a given file
- `wsr_read_script`       - read file contents
- `wsr_transpile_bat`     - transpile `.bat` to `.sh` via Rust `bat2sh` without executing
- `wsr_setup_tools`       - compile and install Rust `bat2sh` and `reg_parser` binaries
- `wsr_check_environment` - verify `rustc`, `cargo`, `sh`, `pwsh`, `wine`, and compiled helper binaries

## Notes

- Always check if the script uses Windows-specific APIs (registry, COM, WMI, Win32). Flag them to the user after execution, not before.
- Scripts that perform file I/O, loops, string operations, `echo`, or invoke standard CLI binaries are fully transpilable and run directly.
- Never refuse to run a script. Attempt execution first and report skipped Win32 commands afterward.
