#!/usr/bin/env python3
# reg_parser.py — Parse Windows .reg and apply Linux equivalents
import sys, re, pathlib, os

content = pathlib.Path(sys.argv[1]).read_text(errors='replace')
key = None; applied=[]; skipped=[]

for line in content.splitlines():
    line = line.strip()
    if not line or line.startswith(';'): continue
    m = re.match(r'\[(.+)\]', line)
    if m: key = m.group(1); continue
    if '=' in line and key:
        n,_,v = line.partition('=')
        n=n.strip().strip('"'); v=v.strip()
        is_env = 'Environment' in key and any(k in key for k in [
            'HKCU', 'HKEY_CURRENT_USER', 'HKLM', 'HKEY_LOCAL_MACHINE'
        ])
        if is_env and not v.startswith(('hex:','dword:','qword:')):
            val = (v.strip('"')
                   .replace('\\','/')
                   .replace('%SystemRoot%','/system')
                   .replace('%USERPROFILE%', os.environ.get('HOME','~'))
                   .replace('%TEMP%','/tmp')
                   .replace('%TMP%','/tmp'))
            bash_line = f'export {n}="{val}"'
            br = pathlib.Path.home()/'.bashrc'
            txt = br.read_text() if br.exists() else ''
            if bash_line not in txt:
                with open(br,'a') as f: f.write(f'\n{bash_line}\n')
            applied.append(f'  {n} = {val}')
        else:
            skipped.append(f'  [{key}] {n} (Windows-only or binary)')

print('=== .reg Parse Report ===')
print(f'File: {sys.argv[1]}')
print(f'\nApplied to ~/.bashrc ({len(applied)}):')
for a in (applied or ['  (none)']): print(a)
print(f'\nSkipped — Windows-only ({len(skipped)}):')
for s in (skipped[:10] or ['  (none)']): print(s)
if len(skipped) > 10: print(f'  ...and {len(skipped)-10} more')
print('\nRun: source ~/.bashrc  to reload env vars')