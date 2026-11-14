# reg_handler

**Purpose**: Parse and interpret Windows `.reg` files on Android. No Windows registry exists on Android — this skill reports what the file would do and applies Linux equivalents where possible.

## Step 1 — Run the parser

```bash
python3 ~/.antigem_tools/reg_parser.py /path/to/file.reg
```

The parser is installed by `wsr_setup_tools`. It reads every key/value pair,
applies what it can to Linux, and prints a full report.

## Step 2 — What gets applied vs skipped

| .reg pattern | What it means | Android action |
|---|---|---|
| `HKCU\Environment\VAR=value` | User env variable | Written to `~/.bashrc` as `export VAR="value"` |
| `HKLM\...\Session Manager\Environment` | System env variable | Written to `~/.bashrc` |
| `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Run` | Startup program | Reported only — no autostart on Android |
| `HKCU\Software\<App>\<Key>` | Per-user app setting | Reported only |
| `HKCR\` (file associations) | File type mappings | Reported only |
| `HKLM\SYSTEM\CurrentControlSet\Services` | Windows service | Reported only |
| `hex:`, `dword:`, `qword:` values | Binary/integer data | Reported only — no conversion |

## Step 3 — Reload env vars if applied

```bash
source ~/.bashrc
```

## Always report to user

After running the parser, tell the user:
- Which keys were written to ~/.bashrc
- Which keys were Windows-only and skipped
- What the full .reg file was intended to do (brief summary)
- Reminder to run `source ~/.bashrc` if any vars were applied

## Manual approach (no parser)

```bash
# Read the file
cat /path/to/file.reg

# For each HKCU\Environment line like:
# "MY_VAR"="my_value"
# Apply manually:
echo 'export MY_VAR="my_value"' >> ~/.bashrc
source ~/.bashrc
```
