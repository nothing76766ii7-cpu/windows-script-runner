# ps1_android

**Purpose**: Run `.ps1` PowerShell scripts on Android via Termux.

## Option A - pkg install (try first)

```bash
pkg update -y
pkg install powershell -y
pwsh --version
pwsh -File /path/to/script.ps1
```

If `pkg install powershell` fails (not in repo for your architecture), use Option B.

## Option B - Download ARM64 binary directly

```bash
mkdir -p ~/powershell && cd ~/powershell
curl -L https://github.com/PowerShell/PowerShell/releases/download/v7.4.6/powershell-7.4.6-linux-arm64.tar.gz      -o pwsh.tar.gz
tar -xzf pwsh.tar.gz
chmod +x ./pwsh
export PATH="$HOME/powershell:$PATH"
echo 'export PATH="$HOME/powershell:$PATH"' >> ~/.bashrc

# Verify
pwsh --version

# Run
pwsh -File /path/to/script.ps1
```

## Option C - POSIX sh transpilation for simple scripts

For scripts that only use variables, `Write-Output`, loops, conditions, and file operations, apply these substitutions:

```text
$var = "x"                    -> export var="x"
Write-Output "x"              -> echo "x"
Write-Host "x"                -> echo "x"
Get-Content file              -> cat file
Set-Content file val          -> echo "$val" > file
Add-Content file val          -> echo "$val" >> file
Remove-Item path              -> rm -rf path
Copy-Item src dst             -> cp -r src dst
Move-Item src dst             -> mv -f src dst
New-Item -Type Directory path -> mkdir -p path
Test-Path path                -> [ -e path ]
if ($x -eq $y)                -> if [ "$x" = "$y" ]
if ($x -ne $y)                -> if [ "$x" != "$y" ]
if ($x -gt $y)                -> if [ "$x" -gt "$y" ]
foreach ($i in $arr)          -> for i in $arr; do ... done
$PSScriptRoot                 -> $(dirname "$(realpath "$0")")
```

## Cmdlets with no Android equivalent - skip and report

`Get-Process`, `Get-Service`, `Get-WmiObject`, `Get-CimInstance`, `Get-EventLog`,
`Start-Service`, `Stop-Service`, `New-PSDrive` (Windows drives),
`Get-ADUser`, `Get-NetAdapter`, `Get-Firewall*`, `Set-ExecutionPolicy`,
`Register-ScheduledTask`, `Get-ScheduledTask`.

Report these to the user. Everything else runs or transpiles.
