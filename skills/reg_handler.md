# reg_handler (Rust Edition)

**Purpose**: Parse and interpret Windows `.reg` files on Android using the native Rust `reg_parser` binary. Because no Windows registry exists on Android, this tool reports what the file would do and applies Linux environment variable equivalents to `~/.bashrc`.

## Step 1 - Run the Rust parser

```bash
~/.antigem_tools/bin/reg_parser /path/to/file.reg
```

The Rust binary `reg_parser` is compiled and installed by `wsr_setup_tools` (from `tools/reg_parser.rs`). It handles both UTF-8 and UTF-16LE `.reg` exports, reads every key/value pair, applies environment variables to `~/.bashrc`, and prints a complete report.

## Step 2 - What gets applied vs skipped

| .reg pattern | What it means | Android action |
|---|---|---|
| `HKCU\Environment\VAR=value` | User env variable | Written to `~/.bashrc` as `export VAR="value"` |
| `HKLM\...\Session Manager\Environment` | System env variable | Written to `~/.bashrc` as `export VAR="value"` |
| `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Run` | Startup program | Reported only - no Windows autostart on Android |
| `HKCU\Software\<App>\<Key>` | Per-user app setting | Reported only |
| `HKCR\` (file associations) | File type mappings | Reported only |
| `HKLM\SYSTEM\CurrentControlSet\Services` | Windows service | Reported only |
| `hex:`, `dword:`, `qword:` values | Binary/integer data | Reported only - no conversion |

## Step 3 - Reload env vars if applied

```bash
. ~/.bashrc
```

## Always report to user

After running the parser, tell the user:
- Which keys were written to `~/.bashrc`
- Which keys were Windows-only and skipped
- What the full `.reg` file was intended to do (brief summary)
- Reminder to reload `~/.bashrc` if any variables were applied
