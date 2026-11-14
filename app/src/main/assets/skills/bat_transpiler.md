# bat_transpiler

**Purpose**: Transpile a `.bat` or `.cmd` file to POSIX shell and execute it in Termux.

## Step 1 — Install transpiler (once)

```bash
mkdir -p ~/.antigem_tools
# worker.js writes bat2sh.py automatically via wsr_setup_tools
# Manual install if needed:
python3 ~/.antigem_tools/bat2sh.py --version 2>/dev/null || echo "run wsr_setup_tools first"
```

## Step 2 — Transpile and run

```bash
# Transpile
python3 ~/.antigem_tools/bat2sh.py /path/to/script.bat

# Execute the resulting .sh file
sh /path/to/script.sh
```

## Transpilation mapping table

| .bat / .cmd              | POSIX sh equivalent                      |
|--------------------------|------------------------------------------|
| `@echo off`              | `set +x`                                 |
| `@echo on`               | `set -x`                                 |
| `echo message`           | `echo "message"`                         |
| `rem comment`            | `# comment`                              |
| `:: comment`             | `# comment`                              |
| `set VAR=value`          | `export VAR="value"`                     |
| `%VAR%`                  | `$VAR`                                   |
| `goto :label`            | `label` (function call)                  |
| `:label`                 | `label() {`                              |
| `if exist path cmd`      | `if [ -e "path" ]; then cmd; fi`         |
| `if not exist path cmd`  | `if [ ! -e "path" ]; then cmd; fi`       |
| `if errorlevel N cmd`    | `if [ $? -ge N ]; then cmd; fi`          |
| `for %%v in (...) do`    | `for v in ...; do`                       |
| `call script.bat`        | `sh script.sh`                           |
| `exit /b 0`              | `exit 0`                                 |
| `pause`                  | `read -r -p 'Press enter...' _p`         |
| `pushd path`             | `pushd path`                             |
| `popd`                   | `popd`                                   |
| `mkdir path` / `md path` | `mkdir -p path`                          |
| `rmdir /s /q path`       | `rm -rf path`                            |
| `copy src dst`           | `cp src dst`                             |
| `move src dst`           | `mv src dst`                             |
| `del file` / `erase`     | `rm -f file`                             |
| `type file`              | `cat file`                               |
| `cd path`                | `cd path`                                |
| `\` (path separator)     | `/`                                      |

## Windows-only commands — skipped with comment

`reg`, `net`, `ipconfig`, `tasklist`, `taskkill`, `sc`, `netstat`, `regedit`,
`msiexec`, `rundll32`, `regsvr32`, `wmic`, `powercfg`, `bcdedit`, `diskpart`,
`icacls`, `attrib`, `cacls`, `takeown`, `cipher`, `compact`, `defrag`,
`chkdsk`, `sfc`, `dism`, `systeminfo`

These are written as `# [SKIP Win32: original line]` and listed in a summary
at the bottom of the transpiled file. Report them to the user after execution.

## Shell compatibility note

Termux's `/bin/sh` is `mksh` or `toybox sh` — not full bash.
Avoid bash-specific: `[[ ]]`, `(( ))`, `${array[@]}`, `source`, `declare`.
Use POSIX: `[ ]`, `expr`, `sh file`.
