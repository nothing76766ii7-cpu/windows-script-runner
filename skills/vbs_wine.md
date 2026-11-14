# vbs_wine

**Purpose**: Run `.vbs` (VBScript) files on Android via Wine inside `proot-distro` Ubuntu.

## Step 1 - Install proot-distro and Ubuntu

```bash
pkg install proot-distro -y
proot-distro install ubuntu
```

## Step 2 - Install Wine inside Ubuntu proot

```bash
proot-distro login ubuntu -- bash -c "
  dpkg --add-architecture i386 &&
  apt update -y &&
  apt install -y wine wine32 wine64 &&
  wineboot --init
"
```

First run takes 2-5 minutes. Subsequent runs are fast.

## Step 3 - Run the .vbs file silently (console host)

```bash
proot-distro login ubuntu -- wine cscript.exe //nologo /path/to/script.vbs
```

`cscript.exe` is the console host and outputs directly to stdout without blocking GUI dialogs. Use `wscript.exe` only if the script explicitly requires GUI `MsgBox` windows.

## What runs reliably

- `WScript.Echo` / `WScript.StdOut.Write`
- `WScript.Sleep`
- `Scripting.FileSystemObject` (file I/O)
- `WScript.Arguments`
- Basic string, math, and array operations
- `WScript.Shell.Run` for standard shell commands

## What fails (Windows internals unsupported on Android)

- `Shell.Application` COM automation
- `WScript.Shell.RegRead` / `RegWrite`
- ActiveX controls
- `CreateObject("Word.Application")` (requires Microsoft Office)

Report unsupported COM objects to the user after attempting execution.
