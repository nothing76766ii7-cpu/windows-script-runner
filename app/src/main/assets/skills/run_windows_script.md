# run_windows_script

**Purpose**: Execute or interpret Windows-only script files on Android via the AntiGem terminal.

## Supported types

| Extension       | Strategy                        | Sub-skill         |
|-----------------|---------------------------------|-------------------|
| `.bat` / `.cmd` | Transpile → `.sh`, run via sh   | `bat_transpiler`  |
| `.ps1`          | PowerShell Core ARM64           | `ps1_android`     |
| `.vbs`          | Wine + wscript.exe              | `vbs_wine`        |
| `.reg`          | Parse + report + apply analog   | `reg_handler`     |
| `.py`           | `python3 <file>` direct         | —                 |
| `.js` / `.mjs`  | `node <file>` direct            | —                 |
| `.sh`           | `chmod +x && sh <file>` direct  | —                 |
| `.exe` / `.msi` | Cannot run — Windows binary     | Report to user    |

## Workflow

1. User provides a file path or pastes script content.
2. Detect extension.
3. Follow the matching sub-skill.
4. Run the resulting command via the terminal MCP tool.
5. Stream and report output.
6. After execution, list any skipped Windows-only lines.

## MCP Tools (from worker.js)

- `wsr_list_directory`   — list files in a path
- `wsr_run_script`       — build + run command for a given file
- `wsr_read_script`      — read file contents
- `wsr_transpile_bat`    — transpile .bat to .sh without executing
- `wsr_setup_tools`      — install bat2sh.py + reg_parser.py
- `wsr_check_environment`— verify sh / python3 / node / pwsh / wine

## Notes

- Always check if the script uses Windows-specific APIs (registry, COM, WMI, Win32).
  These have no Linux equivalent — flag them to the user AFTER execution, not before.
- Scripts that only do file I/O, loops, string ops, echo, or call external binaries
  are fully transpilable and should be run without warning.
- If the user says "just run it" — transpile and execute silently,
  then report skipped lines at the end.
- Never refuse to run a script. Attempt first, report gaps second.
