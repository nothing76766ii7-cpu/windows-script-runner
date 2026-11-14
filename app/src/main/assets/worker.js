/**
 * Windows Script Runner — AntiGem Plugin Worker
 * Registers MCP tools for file listing, script execution, and tool setup.
 * AntiGem calls these tools when Claude invokes them via the Custom Plugin MCP.
 */

// ─── BAT → SH TRANSPILER (embedded, no disk dependency) ────────────────────
const WIN32_CMDS = new Set([
  'reg','net','ipconfig','tasklist','taskkill','sc','netstat','regedit',
  'msiexec','rundll32','regsvr32','wmic','powercfg','bcdedit','diskpart',
  'icacls','attrib','cacls','takeown','cipher','compact','defrag','chkdsk',
  'sfc','dism','systeminfo'
]);

function transpileBat(src) {
  const lines = src.split('\n');
  const out   = ['#!/bin/sh', 'set -e', ''];
  const skipped = [];

  function xlat(line) {
    const l   = line.trim();
    if (!l || /^::/.test(l)) return `# ${l.slice(2).trim()}`;
    const low = l.toLowerCase();

    if (/^@?echo\s+off/i.test(l)) return 'set +x';
    if (/^@?echo\s+on/i.test(l))  return 'set -x';
    if (/^@?echo\./i.test(l))     return 'echo ""';

    let m;
    if ((m = l.match(/^@?echo\s+(.*)/i)))     return `echo "${m[1].replace(/%/g,'$').replace(/"/g,'\\"')}"`;
    if ((m = l.match(/^(?:rem\s+|::)(.*)/i))) return `# ${m[1]}`;
    if ((m = l.match(/^set\s+([^=\s]+)=(.*)/i))) {
      return `export ${m[1].trim()}="${m[2].trim().replace(/%/g,'$')}"`;
    }
    if ((m = l.match(/^goto\s+:?(\w+)/i)))    return m[1].toLowerCase();
    if ((m = l.match(/^:(\w+)/)))             return `\n${m[1].toLowerCase()}() {`;
    if ((m = l.match(/^if\s+exist\s+(\S+)\s+(.*)/i))) {
      return `if [ -e "${m[1].replace(/\\/g,'/')}" ]; then ${xlat(m[2])}; fi`;
    }
    if ((m = l.match(/^if\s+not\s+exist\s+(\S+)\s+(.*)/i))) {
      return `if [ ! -e "${m[1].replace(/\\/g,'/')}" ]; then ${xlat(m[2])}; fi`;
    }
    if ((m = l.match(/^if\s+errorlevel\s+(\d+)\s+(.*)/i))) {
      return `if [ $? -ge ${m[1]} ]; then ${xlat(m[2])}; fi`;
    }
    if ((m = l.match(/^for\s+%%?(\w)\s+in\s+\((.+)\)\s+do\s+(.*)/i))) {
      return `for ${m[1].toLowerCase()} in ${m[2].replace(/,/g,' ')}; do\n  ${xlat(m[3])}\ndone`;
    }
    if ((m = l.match(/^call\s+(.*)/i)))        return `sh ${m[1].replace(/\\/g,'/')}`;
    if ((m = l.match(/^exit\s*(?:\/b)?\s*(\d*)/i))) return `exit ${m[1] || '0'}`;
    if (/^pause/i.test(l))                     return `read -r -p 'Press enter to continue...' _p`;
    if (/^pushd/i.test(l))                     return `pushd ${l.slice(6).trim().replace(/\\/g,'/')}`;
    if (/^popd/i.test(l))                      return 'popd';
    if ((m = l.match(/^(?:mkdir|md)\s+(.*)/i)))  return `mkdir -p ${m[1].replace(/\\/g,'/')}`;
    if ((m = l.match(/^(?:rmdir|rd)(?:\s+\/[sq])*\s+(.*)/i))) return `rm -rf ${m[1].replace(/\\/g,'/')}`;
    if ((m = l.match(/^copy\s+(\S+)\s+(\S+)/i))) return `cp ${m[1].replace(/\\/g,'/')} ${m[2].replace(/\\/g,'/')}`;
    if ((m = l.match(/^move\s+(\S+)\s+(\S+)/i))) return `mv ${m[1].replace(/\\/g,'/')} ${m[2].replace(/\\/g,'/')}`;
    if ((m = l.match(/^(?:del|erase)\s+(.*)/i))) return `rm -f ${m[1].replace(/\\/g,'/')}`;
    if ((m = l.match(/^type\s+(.*)/i)))          return `cat ${m[1].replace(/\\/g,'/')}`;
    if ((m = l.match(/^cd\s*(.*)/i))) {
      const t = m[1].trim().replace(/^\/d\s*/i,'').replace(/\\/g,'/');
      return t ? `cd ${t}` : 'cd ~';
    }

    const cmdName = l.split(/\s+/)[0].toLowerCase().replace(/\.exe$/, '');
    if (WIN32_CMDS.has(cmdName)) {
      skipped.push(l);
      return `# [SKIPPED — Windows-only: ${l}]`;
    }

    return l.replace(/\\/g, '/').replace(/%/g, '$');
  }

  for (const raw of lines) out.push(xlat(raw));

  if (skipped.length) {
    out.push('\n# === SKIPPED Windows-only lines ===');
    skipped.forEach(s => out.push(`# ${s}`));
  }

  return out.join('\n');
}

// ─── COMMAND BUILDER ────────────────────────────────────────────────────────
function buildCommand(filePath, strategy, args = '') {
  const a = args ? ' ' + args : '';
  switch (strategy) {
    case 'bat': {
      const shPath = filePath.replace(/\.[^.]+$/, '.sh');
      return `python3 -c "${transpileBat.toString()}\nopen('${shPath}','w').write(transpileBat(open('${filePath}').read()))" 2>&1 && chmod +x "${shPath}" && sh "${shPath}"${a}`;
    }
    case 'ps1': return `pwsh -File "${filePath}"${a}`;
    case 'vbs': return `proot-distro login ubuntu -- wine wscript.exe "${filePath}"${a}`;
    case 'reg': return buildRegCommand(filePath);
    case 'py':  return `python3 "${filePath}"${a}`;
    case 'js':  return `node "${filePath}"${a}`;
    case 'sh':  return `chmod +x "${filePath}" && sh "${filePath}"${a}`;
    default:    return `"${filePath}"${a}`;
  }
}

function buildRegCommand(filePath) {
  // Inline Python reg parser piped through shell
  return `python3 -c "
import re, pathlib, os
c = pathlib.Path('${filePath}').read_text(errors='replace')
key = None; applied=[]; skipped=[]
for line in c.splitlines():
    line = line.strip()
    if not line or line.startswith(';'): continue
    m = re.match(r'\\\\[(.+)\\\\]', line)
    if m: key=m.group(1); continue
    if '=' in line and key:
        n,_,v = line.partition('=')
        n=n.strip().strip('\"'); v=v.strip()
        if 'Environment' in key and not v.startswith(('hex:','dword:')):
            val=v.strip('\"').replace('%%SystemRoot%%','/system').replace('%%USERPROFILE%%',os.environ.get('HOME','~'))
            line2=f'export {n}=\"{val}\"'
            br=pathlib.Path.home()/'.bashrc'
            txt=br.read_text() if br.exists() else ''
            if line2 not in txt:
                br.open('a').write(f'\\n{line2}\\n')
            applied.append(f'{n}={val}')
        else: skipped.append(f'{key} -> {n}')
print('Applied:',applied or ['none'])
print('Skipped:',skipped[:5],'...' if len(skipped)>5 else '')
" 2>&1`;
}

function detectStrategy(filename) {
  const ext = (filename.match(/\.[^.]+$/)?.[0] || '').toLowerCase();
  return { '.bat':'bat', '.cmd':'bat', '.ps1':'ps1', '.vbs':'vbs',
           '.reg':'reg', '.py':'py', '.js':'js', '.sh':'sh' }[ext] || null;
}

// ─── MCP TOOL DEFINITIONS ───────────────────────────────────────────────────
const MCP_TOOLS = [
  {
    name: 'wsr_list_directory',
    description: 'List files and folders in a directory on the Android filesystem. Returns names, types, sizes, and whether each file is a runnable Windows script.',
    input_schema: {
      type: 'object',
      properties: {
        path: { type: 'string', description: 'Absolute or ~ path to the directory' }
      },
      required: ['path']
    }
  },
  {
    name: 'wsr_run_script',
    description: 'Execute a script file on Android. Handles .bat (transpile→sh), .cmd, .ps1 (PowerShell ARM64), .vbs (Wine), .reg (parse+apply), .py, .js, .sh.',
    input_schema: {
      type: 'object',
      properties: {
        file_path: { type: 'string', description: 'Full path to the script file' },
        args:      { type: 'string', description: 'Optional arguments to pass to the script' }
      },
      required: ['file_path']
    }
  },
  {
    name: 'wsr_read_script',
    description: 'Read the contents of a script file and return as text.',
    input_schema: {
      type: 'object',
      properties: {
        file_path: { type: 'string', description: 'Full path to the script file' }
      },
      required: ['file_path']
    }
  },
  {
    name: 'wsr_transpile_bat',
    description: 'Transpile a .bat or .cmd file to POSIX shell (.sh) without executing it. Returns the transpiled content and a list of skipped Windows-only commands.',
    input_schema: {
      type: 'object',
      properties: {
        file_path: { type: 'string', description: 'Path to the .bat or .cmd file' }
      },
      required: ['file_path']
    }
  },
  {
    name: 'wsr_setup_tools',
    description: 'Install required helper tools for Windows script execution: bat2sh.py, reg_parser.py, and check PowerShell + Wine availability.',
    input_schema: {
      type: 'object',
      properties: {}
    }
  },
  {
    name: 'wsr_check_environment',
    description: 'Check which execution environments are available: sh, python3, node, pwsh (PowerShell), wine (proot-distro Ubuntu).',
    input_schema: {
      type: 'object',
      properties: {}
    }
  }
];

// ─── TOOL HANDLER ────────────────────────────────────────────────────────────
async function handleTool(name, args) {
  switch (name) {

    case 'wsr_list_directory': {
      const path = (args.path || '~').replace('~', process.env.HOME || '/data/data/com.termux/files/home');
      return {
        command: `ls -la "${path}" 2>&1`,
        note: 'Run this command in the terminal MCP to list the directory.'
      };
    }

    case 'wsr_run_script': {
      const strategy = detectStrategy(args.file_path);
      if (!strategy) {
        return { error: `Unsupported file type: ${args.file_path}. Supported: .bat .cmd .ps1 .vbs .reg .py .js .sh` };
      }
      return {
        strategy,
        command: buildCommand(args.file_path, strategy, args.args || ''),
        note: 'Run this command in the terminal MCP.'
      };
    }

    case 'wsr_read_script': {
      return {
        command: `cat "${args.file_path}" 2>&1`,
        note: 'Run in terminal MCP to read the file.'
      };
    }

    case 'wsr_transpile_bat': {
      return {
        command: `cat "${args.file_path}"`,
        post_process: 'Read the file content, transpile with the bat2sh logic from the bat_transpiler skill, write the .sh file, then confirm.',
        note: 'Use the bat_transpiler skill for the full transpilation logic. The worker will also install bat2sh.py via wsr_setup_tools.'
      };
    }

    case 'wsr_setup_tools': {
      return {
        commands: [
          'mkdir -p ~/.antigem_tools',
          `cat > ~/.antigem_tools/bat2sh.py << 'PYEOF'\n${bat2shPython()}\nPYEOF`,
          `cat > ~/.antigem_tools/reg_parser.py << 'PYEOF'\n${regParserPython()}\nPYEOF`,
          'chmod +x ~/.antigem_tools/bat2sh.py ~/.antigem_tools/reg_parser.py',
          'pkg install powershell -y 2>/dev/null || echo "PowerShell: not in pkg, download manually from GitHub ARM64 release"',
          'pkg install proot-distro -y 2>/dev/null && echo "proot-distro: OK" || echo "proot-distro: install manually"',
          'python3 --version && node --version && pwsh --version 2>/dev/null || true'
        ],
        note: 'Run each command in the terminal MCP in order.'
      };
    }

    case 'wsr_check_environment': {
      return {
        command: [
          'echo "=== Environment Check ==="',
          'which sh && sh --version 2>&1 | head -1 || echo "sh: missing"',
          'which python3 && python3 --version || echo "python3: missing"',
          'which node && node --version || echo "node: missing"',
          'which pwsh && pwsh --version || echo "pwsh: not installed (pkg install powershell)"',
          'which wine 2>/dev/null && wine --version || echo "wine: not installed (proot-distro login ubuntu -- apt install wine)"',
          'proot-distro list 2>/dev/null || echo "proot-distro: not installed"',
          'ls ~/.antigem_tools/ 2>/dev/null || echo "Helper tools not installed (use wsr_setup_tools)"'
        ].join(' && '),
        note: 'Run in terminal MCP.'
      };
    }

    default:
      return { error: `Unknown tool: ${name}` };
  }
}

// ─── EMBEDDED PYTHON SCRIPTS ────────────────────────────────────────────────
function bat2shPython() {
  return `#!/usr/bin/env python3
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
    if re.match(r'@?echo\\s+off',low): return 'set +x'
    if re.match(r'@?echo\\s+on', low): return 'set -x'
    m = re.match(r'@?echo\\s+(.*)',l,re.I)
    if m: return f'echo "{m.group(1).replace("%","$").replace(chr(34),chr(92)+chr(34))}"'
    m = re.match(r'(?:rem\\s+|::)(.*)',l,re.I)
    if m: return f'# {m.group(1)}'
    m = re.match(r'set\\s+([^=\\s]+)=(.*)',l,re.I)
    if m: return f'export {m.group(1).strip()}="{m.group(2).strip().replace("%","$")}"'
    m = re.match(r'goto\\s+:?(\\w+)',l,re.I)
    if m: return m.group(1).lower()
    m = re.match(r':(\\w+)',l)
    if m: return f'\\n{m.group(1).lower()}() {{'
    m = re.match(r'if\\s+exist\\s+(\\S+)\\s+(.*)',l,re.I)
    if m: return f'if [ -e "{m.group(1).replace(chr(92),chr(47))}" ]; then {xlat(m.group(2))}; fi'
    m = re.match(r'if\\s+not\\s+exist\\s+(\\S+)\\s+(.*)',l,re.I)
    if m: return f'if [ ! -e "{m.group(1).replace(chr(92),chr(47))}" ]; then {xlat(m.group(2))}; fi'
    m = re.match(r'if\\s+errorlevel\\s+(\\d+)\\s+(.*)',l,re.I)
    if m: return f'if [ $? -ge {m.group(1)} ]; then {xlat(m.group(2))}; fi'
    m = re.match(r'for\\s+%%?(\\w)\\s+in\\s+\\((.+)\\)\\s+do\\s+(.*)',l,re.I)
    if m: return f'for {m.group(1).lower()} in {m.group(2).replace(",", " ")}; do\\n  {xlat(m.group(3))}\\ndone'
    m = re.match(r'call\\s+(.*)',l,re.I)
    if m: return f'sh {m.group(1).replace(chr(92),chr(47))}'
    m = re.match(r'exit\\s*(?:/b)?\\s*(\\d*)',l,re.I)
    if m: return f'exit {m.group(1) or "0"}'
    if re.match(r'pause',low): return "read -r -p 'Press enter...' _p"
    if re.match(r'pushd',low): return f'pushd {l[6:].strip().replace(chr(92),chr(47))}'
    if re.match(r'popd', low): return 'popd'
    m = re.match(r'(?:mkdir|md)\\s+(.*)',l,re.I)
    if m: return f'mkdir -p {m.group(1).replace(chr(92),chr(47))}'
    m = re.match(r'(?:rmdir|rd)(?:\\s+/[sq])*\\s+(.*)',l,re.I)
    if m: return f'rm -rf {m.group(1).replace(chr(92),chr(47))}'
    m = re.match(r'copy\\s+(\\S+)\\s+(\\S+)',l,re.I)
    if m: return f'cp {m.group(1).replace(chr(92),chr(47))} {m.group(2).replace(chr(92),chr(47))}'
    m = re.match(r'move\\s+(\\S+)\\s+(\\S+)',l,re.I)
    if m: return f'mv {m.group(1).replace(chr(92),chr(47))} {m.group(2).replace(chr(92),chr(47))}'
    m = re.match(r'(?:del|erase)\\s+(.*)',l,re.I)
    if m: return f'rm -f {m.group(1).replace(chr(92),chr(47))}'
    m = re.match(r'type\\s+(.*)',l,re.I)
    if m: return f'cat {m.group(1).replace(chr(92),chr(47))}'
    m = re.match(r'cd\\s*(.*)',l,re.I)
    if m:
        t = re.sub(r'^/d\\s*','',m.group(1).strip(),flags=re.I).replace(chr(92),chr(47))
        return f'cd {t}' if t else 'cd ~'
    cn = l.split()[0].lower().rstrip('.exe') if l.split() else ''
    if cn in WIN32: return f'# [SKIP Win32: {l}]'
    return l.replace(chr(92), chr(47)).replace('%', '$')

if __name__ == '__main__':
    src = pathlib.Path(sys.argv[1]).read_text(errors='replace')
    out = ['#!/bin/sh','set -e','']
    for raw in src.splitlines(): out.append(xlat(raw))
    result = '\\n'.join(out)
    p = pathlib.Path(sys.argv[1]).with_suffix('.sh')
    p.write_text(result)
    print(f'Written: {p}')
    skipped = [l for l in result.splitlines() if l.startswith('# [SKIP')]
    if skipped:
        print(f'\\nSkipped {len(skipped)} Windows-only lines:')
        for s in skipped: print(f'  {s}')`;
}

function regParserPython() {
  return `#!/usr/bin/env python3
# reg_parser.py — Parse Windows .reg and apply Linux equivalents
import sys, re, pathlib, os

content = pathlib.Path(sys.argv[1]).read_text(errors='replace')
key = None; applied=[]; skipped=[]

for line in content.splitlines():
    line = line.strip()
    if not line or line.startswith(';'): continue
    m = re.match(r'\\[(.+)\\]', line)
    if m: key = m.group(1); continue
    if '=' in line and key:
        n,_,v = line.partition('=')
        n=n.strip().strip('"'); v=v.strip()
        is_env = 'Environment' in key and any(k in key for k in [
            'HKCU', 'HKEY_CURRENT_USER', 'HKLM', 'HKEY_LOCAL_MACHINE'
        ])
        if is_env and not v.startswith(('hex:','dword:','qword:')):
            val = (v.strip('"')
                   .replace('\\\\','/')
                   .replace('%SystemRoot%','/system')
                   .replace('%USERPROFILE%', os.environ.get('HOME','~'))
                   .replace('%TEMP%','/tmp')
                   .replace('%TMP%','/tmp'))
            bash_line = f'export {n}="{val}"'
            br = pathlib.Path.home()/'.bashrc'
            txt = br.read_text() if br.exists() else ''
            if bash_line not in txt:
                with open(br,'a') as f: f.write(f'\\n{bash_line}\\n')
            applied.append(f'  {n} = {val}')
        else:
            skipped.append(f'  [{key}] {n} (Windows-only or binary)')

print('=== .reg Parse Report ===')
print(f'File: {sys.argv[1]}')
print(f'\\nApplied to ~/.bashrc ({len(applied)}):')
for a in (applied or ['  (none)']): print(a)
print(f'\\nSkipped — Windows-only ({len(skipped)}):')
for s in (skipped[:10] or ['  (none)']): print(s)
if len(skipped) > 10: print(f'  ...and {len(skipped)-10} more')
print('\\nRun: source ~/.bashrc  to reload env vars')`;
}

// ─── EXTENSION HOST BRIDGE ───────────────────────────────────────────────────
// AntiGem injects the chrome.runtime API into the plugin worker context.
// The host calls onMessage with { type: 'MCP_TOOL_CALL', tool, args }
// and expects a response via sendResponse.

if (typeof self !== 'undefined') {
  // Service worker context (AntiGem plugin host)
  self.addEventListener('message', async (event) => {
    const { type, tool, args, id } = event.data || {};

    if (type === 'MCP_TOOLS_LIST') {
      event.source?.postMessage({ id, tools: MCP_TOOLS });
      return;
    }

    if (type === 'MCP_TOOL_CALL') {
      const result = await handleTool(tool, args || {});
      event.source?.postMessage({ id, result });
      return;
    }

    if (type === 'RUN_COMMAND') {
      // UI panel → worker → terminal MCP bridge
      // AntiGem provides window.antigem.terminal in the worker context
      try {
        const { cmd } = event.data;
        const result = await self.antigem?.terminal?.run?.(cmd) ?? { stdout: '', stderr: '', exitCode: 0 };
        event.source?.postMessage({
          id,
          output: (result.stdout || '') + (result.stderr ? '\n[ERR] ' + result.stderr : ''),
          exit: result.exitCode ?? 0
        });
      } catch (e) {
        event.source?.postMessage({ id, output: `[ERROR] ${e.message}`, exit: 1 });
      }
      return;
    }

    if (type === 'LIST_DIR') {
      try {
        const result = await self.antigem?.terminal?.run?.(`ls -la "${event.data.path}" 2>&1`) ?? { stdout: '' };
        event.source?.postMessage({ id, raw: result.stdout });
      } catch (e) {
        event.source?.postMessage({ id, raw: '', error: e.message });
      }
      return;
    }
  });
}

// chrome.runtime path (older AntiGem plugin API)
if (typeof chrome !== 'undefined' && chrome.runtime?.onMessage) {
  chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
    const { type, tool, args } = message;

    if (type === 'MCP_TOOLS_LIST') {
      sendResponse({ tools: MCP_TOOLS });
      return true;
    }

    if (type === 'MCP_TOOL_CALL') {
      handleTool(tool, args || {}).then(result => sendResponse({ result }));
      return true; // async
    }

    if (type === 'RUN_COMMAND') {
      (self.antigem?.terminal?.run?.(message.cmd) ?? Promise.resolve({ stdout:'', exitCode:0 }))
        .then(r => sendResponse({
          output: (r.stdout||'') + (r.stderr ? '\n' + r.stderr : ''),
          exit: r.exitCode ?? 0
        }))
        .catch(e => sendResponse({ output: `[ERROR] ${e.message}`, exit: 1 }));
      return true;
    }

    if (type === 'LIST_DIR') {
      (self.antigem?.terminal?.run?.(`ls -la "${message.path}" 2>&1`) ?? Promise.resolve({ stdout: '' }))
        .then(r => sendResponse({ raw: r.stdout }))
        .catch(e => sendResponse({ raw: '', error: e.message }));
      return true;
    }
  });
}

// Export for AntiGem's direct MCP registration path
if (typeof module !== 'undefined') {
  module.exports = { MCP_TOOLS, handleTool, transpileBat, detectStrategy };
}
