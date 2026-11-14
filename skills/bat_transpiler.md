# bat_transpiler (Rust Edition)

**Purpose**: Transpile a `.bat` or `.cmd` file to POSIX shell (`.sh`) using the native Rust `bat2sh` compiler and execute it in Termux.

## Step 1 - Compile & install the Rust transpiler (once)

```bash
mkdir -p ~/.antigem_tools/src ~/.antigem_tools/bin
pkg install rust -y 2>/dev/null || true
rustc tools/bat2sh.rs -O -o ~/.antigem_tools/bin/bat2sh
chmod +x ~/.antigem_tools/bin/bat2sh
~/.antigem_tools/bin/bat2sh --version
```

## Step 2 - Transpile and run

```bash
# Transpile via native Rust binary
~/.antigem_tools/bin/bat2sh /path/to/script.bat

# Execute the resulting .sh file
chmod +x /path/to/script.sh
sh /path/to/script.sh
```

## Transpilation mapping table

| .bat / .cmd              | POSIX sh equivalent                              |
|--------------------------|--------------------------------------------------|
| `@echo off`              | `set +x`                                         |
| `@echo on`               | `set -x`                                         |
| `echo.` / `echo:`        | `echo ""`                                        |
| `echo message`           | `echo "message"`                                 |
| `rem comment`            | `# comment`                                      |
| `:: comment`             | `# comment`                                      |
| `cls`                    | `clear`                                          |
| `set VAR=value`          | `export VAR="value"`                             |
| `set /a VAR=expr`        | `export VAR=$(( expr ))`                         |
| `set /p VAR="Prompt"`    | `printf '%s' "Prompt"; read -r VAR; export VAR`  |
| `%VAR%`                  | `${VAR}`                                         |
| `%1` .. `%9`             | `$1` .. `$9`                                     |
| `%ERRORLEVEL%`           | `$?`                                             |
| `goto :label`            | `label` (function call)                          |
| `goto :eof`              | `exit 0`                                         |
| `:label`                 | `label() {`                                      |
| `if exist path cmd`      | `if [ -e "path" ]; then cmd; fi`                 |
| `if not exist path cmd`  | `if [ ! -e "path" ]; then cmd; fi`               |
| `if errorlevel N cmd`    | `if [ $? -ge N ]; then cmd; fi`                  |
| `for %%v in (...) do`    | `for v in ...; do`                               |
| `call script.bat`        | `sh script.sh`                                   |
| `exit /b 0`              | `exit 0`                                         |
| `pause`                  | `printf 'Press enter to continue...'; read -r _p`|
| `timeout /t N`           | `sleep N`                                        |
| `pushd path`             | `pushd path`                                     |
| `popd`                   | `popd`                                           |
| `mkdir path` / `md path` | `mkdir -p path`                                  |
| `rmdir /s /q path`       | `rm -rf path`                                    |
| `copy src dst`           | `cp -r src dst`                                  |
| `move src dst`           | `mv -f src dst`                                  |
| `del file` / `erase`     | `rm -f file`                                     |
| `type file`              | `cat file`                                       |
| `cd /d path`             | `cd path`                                        |
| `\` (path separator)     | `/`                                              |

## Windows-only commands - skipped with comment

`reg`, `net`, `ipconfig`, `tasklist`, `taskkill`, `sc`, `netstat`, `regedit`,
`msiexec`, `rundll32`, `regsvr32`, `wmic`, `powercfg`, `bcdedit`, `diskpart`,
`icacls`, `attrib`, `cacls`, `takeown`, `cipher`, `compact`, `defrag`,
`chkdsk`, `sfc`, `dism`, `systeminfo`

These are written as `# [SKIP Win32: original line]` and listed in a summary
at the bottom of the transpiled file. Report them to the user after execution.

## Shell compatibility note

Termux's `/bin/sh` is `mksh` or `toybox sh` - not full bash.
Avoid bash-specific syntax: `[[ ]]`, `${array[@]}`, `source`, `declare`.
Use POSIX constructs: `[ ]`, `$(( ))`, `sh file`.
