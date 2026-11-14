#!/usr/bin/env python3
# bat2sh.py — .bat/.cmd to POSIX sh transpiler for AntiGem
import sys, re, pathlib

WIN32 = {"reg","net","ipconfig","tasklist","taskkill","sc","netstat","regedit",
         "msiexec","rundll32","regsvr32","wmic","powercfg","bcdedit","diskpart",
         "icacls","attrib","cacls","takeown","cipher","compact","defrag",
         "chkdsk","sfc","dism","systeminfo"}

def xlat(line):
    l = line.strip()
    if not l or l.startswith('::'): return f"# {l[2:].strip()}"
    low = l.lower()
    if re.match(r'@?echo\s+off',low): return 'set +x'
    if re.match(r'@?echo\s+on', low): return 'set -x'
    m = re.match(r'@?echo\s+(.*)',l,re.I)
    if m: return f'echo "{m.group(1).replace("%","$").replace(chr(34),chr(92)+chr(34))}"'
    m = re.match(r'(?:rem\s+|::)(.*)',l,re.I)
    if m: return f'# {m.group(1)}'
    m = re.match(r'set\s+([^=\s]+)=(.*)',l,re.I)
    if m: return f'export {m.group(1).strip()}="{m.group(2).strip().replace("%","$")}"'
    m = re.match(r'goto\s+:?(\w+)',l,re.I)
    if m: return m.group(1).lower()
    m = re.match(r':(\w+)',l)
    if m: return f'\n{m.group(1).lower()}() {{'
    m = re.match(r'if\s+exist\s+(\S+)\s+(.*)',l,re.I)
    if m: return f'if [ -e "{m.group(1).replace(chr(92),chr(47))}" ]; then {xlat(m.group(2))}; fi'
    m = re.match(r'if\s+not\s+exist\s+(\S+)\s+(.*)',l,re.I)
    if m: return f'if [ ! -e "{m.group(1).replace(chr(92),chr(47))}" ]; then {xlat(m.group(2))}; fi'
    m = re.match(r'if\s+errorlevel\s+(\d+)\s+(.*)',l,re.I)
    if m: return f'if [ $? -ge {m.group(1)} ]; then {xlat(m.group(2))}; fi'
    m = re.match(r'for\s+%%?(\w)\s+in\s+\((.+)\)\s+do\s+(.*)',l,re.I)
    if m: return f'for {m.group(1).lower()} in {m.group(2).replace(",", " ")}; do\n  {xlat(m.group(3))}\ndone'
    m = re.match(r'call\s+(.*)',l,re.I)
    if m: return f'sh {m.group(1).replace(chr(92),chr(47))}'
    m = re.match(r'exit\s*(?:/b)?\s*(\d*)',l,re.I)
    if m: return f'exit {m.group(1) or "0"}'
    if re.match(r'pause',low): return "read -r -p 'Press enter...' _p"
    if re.match(r'pushd',low): return f'pushd {l[6:].strip().replace(chr(92),chr(47))}'
    if re.match(r'popd', low): return 'popd'
    m = re.match(r'(?:mkdir|md)\s+(.*)',l,re.I)
    if m: return f'mkdir -p {m.group(1).replace(chr(92),chr(47))}'
    m = re.match(r'(?:rmdir|rd)(?:\s+/[sq])*\s+(.*)',l,re.I)
    if m: return f'rm -rf {m.group(1).replace(chr(92),chr(47))}'
    m = re.match(r'copy\s+(\S+)\s+(\S+)',l,re.I)
    if m: return f'cp {m.group(1).replace(chr(92),chr(47))} {m.group(2).replace(chr(92),chr(47))}'
    m = re.match(r'move\s+(\S+)\s+(\S+)',l,re.I)
    if m: return f'mv {m.group(1).replace(chr(92),chr(47))} {m.group(2).replace(chr(92),chr(47))}'
    m = re.match(r'(?:del|erase)\s+(.*)',l,re.I)
    if m: return f'rm -f {m.group(1).replace(chr(92),chr(47))}'
    m = re.match(r'type\s+(.*)',l,re.I)
    if m: return f'cat {m.group(1).replace(chr(92),chr(47))}'
    m = re.match(r'cd\s*(.*)',l,re.I)
    if m:
        t = re.sub(r'^/d\s*','',m.group(1).strip(),flags=re.I).replace(chr(92),chr(47))
        return f'cd {t}' if t else 'cd ~'
    cn = l.split()[0].lower().rstrip('.exe') if l.split() else ''
    if cn in WIN32: return f'# [SKIP Win32: {l}]'
    return l.replace(chr(92), chr(47)).replace('%', '$')

if __name__ == '__main__':
    src = pathlib.Path(sys.argv[1]).read_text(errors='replace')
    out = ['#!/bin/sh','set -e','']
    for raw in src.splitlines(): out.append(xlat(raw))
    result = '\n'.join(out)
    p = pathlib.Path(sys.argv[1]).with_suffix('.sh')
    p.write_text(result)
    print(f'Written: {p}')
    skipped = [l for l in result.splitlines() if l.startswith('# [SKIP')]
    if skipped:
        print(f'\nSkipped {len(skipped)} Windows-only lines:')
        for s in skipped: print(f'  {s}')