# vbs_wine

**Purpose**: Run `.vbs` (VBScript) files on Android via Wine inside proot-distro Ubuntu.

## Step 1 — Install proot-distro and Ubuntu

```bash
pkg install proot-distro -y
proot-distro install ubuntu
```

## Step 2 — Install Wine inside Ubuntu proot

```bash
proot-distro login ubuntu -- bash -c "
  dpkg --add-architecture i386 &&
  apt update -y &&
  apt install -y wine wine32 wine64 &&
  wineboot --init
"
```

First run takes ~2–5 minutes. Subsequent runs are fast.

## Step 3 — Run the .vbs file

```bash
proot-distro login ubuntu -- wine wscript.exe /path/to/script.vbs
```

If the path is inside Termux home:

```bash
proot-distro login ubuntu -- bash -c "
  cp /data/data/com.termux/files/home/script.vbs /root/ &&
  wine wscript.exe /root/script.vbs
"
```

## Step 4 — Run silently (no GUI dialogs)

```bash
proot-distro login ubuntu -- wine cscript.exe //nologo /path/to/script.vbs
```

`cscript.exe` is the console host — outputs to stdout, no dialog boxes.
Use this by default unless the script explicitly uses `MsgBox`.

## What runs fine

- `WScript.Echo` / `WScript.StdOut.Write`
- `WScript.Sleep`
- `Scripting.FileSystemObject` (file I/O)
- `WScript.Arguments`
- Basic string, math, array ops
- `WScript.Shell.Run` for simple shell commands

## What fails (Windows internals Wine may not support)

- `Shell.Application` COM automation
- `WScript.Shell.RegRead` / `RegWrite` — registry (no Windows registry)
- ActiveX controls
- `CreateObject("ADODB.*")` — depends on Wine's COM implementation
- `CreateObject("Word.Application")` — requires MS Office in Wine

Report unsupported objects to the user. Do not block execution —
attempt first, report what failed from the output.

## Lightweight alternative for WScript.Echo-only scripts

```bash
# pyvbscript handles simple VBS without Wine
pip install pyvbscript --break-system-packages 2>/dev/null
python3 -c "import pyvbscript; pyvbscript.run(open('/path/to/script.vbs').read())"
```
