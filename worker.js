/**
 * Windows Script Runner (Rust VFS + PE Edition v3.0) - AntiGem Plugin Worker Bridge
 * Supports Windows VFS (C:\, D:\, Z:\), .bat/.cmd transpilation, .exe/.msi PE execution via Box64/Wine,
 * .ps1, .vbs, .reg, and native .rs scripts. Zero emojis used anywhere.
 */

const WIN32_CMDS = new Set([
  'reg','net','sc','netstat','regedit',
  'rundll32','regsvr32','wmic','powercfg','bcdedit','diskpart',
  'icacls','attrib','cacls','takeown','cipher','compact','defrag','chkdsk',
  'sfc','dism'
]);

function translateWinPath(input) {
  return String(input)
    .replace(/%[Uu][Ss][Ee][Rr][Pp][Rr][Oo][Ff][Ii][Ll][Ee]%/g, '${USERPROFILE}')
    .replace(/%[Ss][Yy][Ss][Tt][Ee][Mm][Rr][Oo][Oo][Tt]%/g, '${SystemRoot}')
    .replace(/%[Tt][Ee][Mm][Pp]%/g, '${TEMP}')
    .replace(/%[Tt][Mm][Pp]%/g, '${TMP}')
    .replace(/%([^%]+)%/g, '${$1}')
    .replace(/[Cc]:\\/g, '${WSR_DRIVE_C}/')
    .replace(/[Cc]:\//g, '${WSR_DRIVE_C}/')
    .replace(/[Dd]:\\/g, '${WSR_DRIVE_D}/')
    .replace(/[Dd]:\//g, '${WSR_DRIVE_D}/')
    .replace(/[Zz]:\\/g, '/')
    .replace(/[Zz]:\//g, '/')
    .replace(/\\/g, '/');
}

function transpileBat(src) {
  const lines = src.split(/\r?\n/);
  const out = [
    '#!/bin/sh',
    '# Transpiled by Windows Script Runner (Rust VFS Edition v3.0)',
    'set -e',
    'export WSR_DRIVE_C="${HOME}/.wsr_vfs/drive_c"',
    'export WSR_DRIVE_D="/sdcard"',
    'export WSR_DRIVE_Z="/"',
    'export USERPROFILE="${WSR_DRIVE_C}/Users/Admin"',
    'export SystemRoot="${WSR_DRIVE_C}/Windows"',
    'export TEMP="${WSR_DRIVE_C}/Users/Admin/AppData/Local/Temp"',
    'export TMP="${TEMP}"',
    'mkdir -p "${USERPROFILE}/Desktop" "${SystemRoot}/System32" "${TEMP}"',
    ''
  ];
  const skipped = [];
  let openFunc = false;

  function xlat(line) {
    const l = line.trim();
    if (!l) return '';
    if (/^::/.test(l)) return `# ${l.slice(2).trim()}`;
    const clean = l.replace(/^@/, '').trim();
    const low = clean.toLowerCase();

    if (/^[Cc]:$/.test(clean)) return 'cd "${WSR_DRIVE_C}"';
    if (/^[Dd]:$/.test(clean)) return 'cd "${WSR_DRIVE_D}"';
    if (/^[Zz]:$/.test(clean)) return 'cd /';
    if (/^echo\s+off/i.test(clean)) return 'set +x';
    if (/^echo\s+on/i.test(clean))  return 'set -x';
    if (/^echo[.:]/i.test(clean))    return 'echo ""';

    let m;
    if ((m = clean.match(/^echo\s+(.*)/i))) return `echo "${translateWinPath(m[1]).replace(/"/g,'\\"')}"`;
    if ((m = clean.match(/^rem(?:\s+(.*))?$/i))) return `# ${m[1] || ''}`;
    if (low === 'cls') return 'clear';
    if (low === 'ver') return 'echo "Microsoft Windows [Version 10.0.22631.4317 - Android Rust VFS]"';
    if (low === 'dir' || low.startsWith('dir ')) {
      const arg = clean.slice(3).trim();
      return arg ? `ls -la ${translateWinPath(arg)}` : 'ls -la';
    }
    if ((m = clean.match(/^set\s+\/a\s+([^=\s]+)=(.*)/i))) {
      return `export ${m[1].trim()}=$(( ${translateWinPath(m[2].trim())} ))`;
    }
    if ((m = clean.match(/^set\s+\/p\s+([^=\s]+)=(.*)/i))) {
      const v = m[1].trim();
      const p = m[2].trim().replace(/^"|"$/g, '');
      return `printf '%s' "${p}"; read -r ${v}; export ${v}`;
    }
    if ((m = clean.match(/^set\s+([^=\s]+)=(.*)/i))) {
      return `export ${m[1].trim()}="${translateWinPath(m[2].trim().replace(/^"|"$/g,''))}"`;
    }
    if ((m = clean.match(/^goto\s+:?(\w+)/i))) {
      const lbl = m[1].toLowerCase();
      return lbl === 'eof' ? 'exit 0' : lbl;
    }
    if ((m = clean.match(/^:(\w+)/))) {
      const prefix = openFunc ? '}\n\n' : '\n';
      openFunc = true;
      return `${prefix}${m[1].toLowerCase()}() {`;
    }
    if ((m = clean.match(/^if\s+not\s+exist\s+(\S+)\s+(.*)/i))) {
      return `if [ ! -e "${translateWinPath(m[1].replace(/^"|"$/g,''))}" ]; then ${xlat(m[2])}; fi`;
    }
    if ((m = clean.match(/^if\s+exist\s+(\S+)\s+(.*)/i))) {
      return `if [ -e "${translateWinPath(m[1].replace(/^"|"$/g,''))}" ]; then ${xlat(m[2])}; fi`;
    }
    if ((m = clean.match(/^if\s+errorlevel\s+(\d+)\s+(.*)/i))) {
      return `if [ $? -ge ${m[1]} ]; then ${xlat(m[2])}; fi`;
    }
    if ((m = clean.match(/^for\s+%%?(\w)\s+in\s+\((.+)\)\s+do\s+(.*)/i))) {
      return `for ${m[1].toLowerCase()} in ${translateWinPath(m[2].replace(/,/g,' '))}; do\n  ${xlat(m[3])}\ndone`;
    }
    if ((m = clean.match(/^call\s+(.*)/i))) return `sh ${translateWinPath(m[1]).replace(/\.(bat|cmd)$/i,'.sh')}`;
    if ((m = clean.match(/^exit\s*(?:\/b)?\s*(\d*)/i))) return `exit ${m[1] || '0'}`;
    if (/^pause/i.test(clean)) return `printf 'Press enter to continue...'; read -r _p`;
    if (/^pushd/i.test(clean)) return `pushd ${translateWinPath(clean.slice(6).trim())}`;
    if (/^popd/i.test(clean))  return 'popd';
    if ((m = clean.match(/^(?:mkdir|md)\s+(.*)/i))) return `mkdir -p ${translateWinPath(m[1])}`;
    if ((m = clean.match(/^(?:rmdir|rd)(?:\s+\/[sq])*\s+(.*)/i))) return `rm -rf ${translateWinPath(m[1])}`;
    if ((m = clean.match(/^(?:copy|xcopy)\s+(.*)/i))) return `cp -r ${translateWinPath(m[1])}`;
    if ((m = clean.match(/^move\s+(.*)/i))) return `mv -f ${translateWinPath(m[1])}`;
    if ((m = clean.match(/^(?:del|erase)(?:\s+\/[fqs])*\s+(.*)/i))) return `rm -f ${translateWinPath(m[1])}`;
    if ((m = clean.match(/^type\s+(.*)/i))) return `cat ${translateWinPath(m[1])}`;
    if ((m = clean.match(/^cd\s*(.*)/i))) {
      const t = translateWinPath(m[1].trim().replace(/^\/d\s*/i,''));
      return t ? `cd ${t}` : 'cd "${USERPROFILE}"';
    }

    const cmdName = clean.split(/\s+/)[0].toLowerCase().replace(/\.exe$/, '');
    if (WIN32_CMDS.has(cmdName)) {
      skipped.push(l);
      return `# [SKIP Win32: ${l}]`;
    }

    return translateWinPath(clean);
  }

  for (const raw of lines) out.push(xlat(raw));
  if (openFunc) out.push('}');

  if (skipped.length) {
    out.push('\n# === SKIPPED Windows-only lines ===');
    skipped.forEach(s => out.push(`# ${s}`));
  }

  return out.join('\n') + '\n';
}

function buildCommand(filePath, strategy, args = '') {
  const a = args ? ' ' + args : '';
  const posixPath = translateWinPath(filePath);
  const shPath = posixPath.replace(/\.[^.]+$/, '.sh');
  switch (strategy) {
    case 'bat':
      return `~/.antigem_tools/bin/bat2sh "${posixPath}" && chmod +x "${shPath}" && sh "${shPath}"${a}`;
    case 'ps1':
      return `pwsh -File "${posixPath}"${a}`;
    case 'vbs':
      return `proot-distro login ubuntu -- wine cscript.exe //nologo "${posixPath}"${a}`;
    case 'reg':
      return `~/.antigem_tools/bin/reg_parser "${posixPath}"`;
    case 'exe':
      return `if command -v box64 >/dev/null 2>&1 && command -v wine64 >/dev/null 2>&1; then WINEDEBUG=-all box64 wine64 "${posixPath}"${a}; elif command -v wine >/dev/null 2>&1; then WINEDEBUG=-all wine "${posixPath}"${a}; elif command -v proot-distro >/dev/null 2>&1; then proot-distro login ubuntu -- wine "${posixPath}"${a}; else ~/.antigem_tools/bin/wsr pe "${posixPath}"; fi`;
    case 'msi':
      return `if command -v wine >/dev/null 2>&1; then wine msiexec /i "${posixPath}"${a}; else proot-distro login ubuntu -- wine msiexec /i "${posixPath}"${a}; fi`;
    case 'rs':
      return `rustc "${posixPath}" -O -o /tmp/wsr_rust_bin && /tmp/wsr_rust_bin${a}`;
    case 'py':
      return `python3 "${posixPath}"${a}`;
    case 'js':
      return `node "${posixPath}"${a}`;
    case 'sh':
      return `chmod +x "${posixPath}" && sh "${posixPath}"${a}`;
    default:
      return `"${posixPath}"${a}`;
  }
}

function detectStrategy(filename) {
  const ext = (filename.match(/\.[^.]+$/)?.[0] || '').toLowerCase();
  return {
    '.bat': 'bat',
    '.cmd': 'bat',
    '.ps1': 'ps1',
    '.vbs': 'vbs',
    '.reg': 'reg',
    '.exe': 'exe',
    '.msi': 'msi',
    '.rs':  'rs',
    '.py':  'py',
    '.js':  'js',
    '.mjs': 'js',
    '.sh':  'sh'
  }[ext] || null;
}

const MCP_TOOLS = [
  {
    name: 'wsr_list_directory',
    description: 'List files and folders in a Windows VFS drive (C:\, D:\, Z:\) or Android directory.',
    input_schema: {
      type: 'object',
      properties: {
        path: { type: 'string', description: 'Windows path (e.g. C:\Users\Admin\Desktop) or POSIX path' }
      },
      required: ['path']
    }
  },
  {
    name: 'wsr_run_script',
    description: 'Execute a Windows or POSIX script/binary on Android (.bat, .cmd, .exe, .msi, .ps1, .vbs, .reg, .rs, .py, .js, .sh).',
    input_schema: {
      type: 'object',
      properties: {
        file_path: { type: 'string', description: 'Full path to the script or .exe' },
        args:      { type: 'string', description: 'Optional arguments to pass to the script' }
      },
      required: ['file_path']
    }
  },
  {
    name: 'wsr_inspect_exe',
    description: 'Inspect a Windows PE (.exe, .dll, .msi) file header for CPU architecture, subsystem, and .NET CLR support.',
    input_schema: {
      type: 'object',
      properties: {
        file_path: { type: 'string', description: 'Path to the .exe or .dll file' }
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
    description: 'Transpile a .bat or .cmd file to POSIX shell (.sh) with Windows VFS path translation.',
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
    description: 'Initialize the Windows VFS (C:\, D:\, Z:\) and compile the native Rust helper binaries (bat2sh, reg_parser).',
    input_schema: {
      type: 'object',
      properties: {}
    }
  },
  {
    name: 'wsr_check_environment',
    description: 'Check Windows VFS mounts and availability of rustc, cargo, box64, wine, mono, pwsh, and helper binaries.',
    input_schema: {
      type: 'object',
      properties: {}
    }
  }
];

function bat2shRustSource() {
  return "#!/usr/bin/env rust-script\n//! Standalone zero-dependency Rust `.bat` / `.cmd` to POSIX `.sh` transpiler with Windows VFS (`C:\\`, `D:\\`, `Z:\\`).\n//! Compile directly with: `rustc tools/bat2sh.rs -O -o ~/.antigem_tools/bin/bat2sh`\n\nuse std::env;\nuse std::fs;\nuse std::path::Path;\nuse std::process;\n\nconst WIN32_CMDS: &[&str] = &[\n    \"reg\", \"net\", \"sc\", \"netstat\", \"regedit\",\n    \"rundll32\", \"regsvr32\", \"wmic\", \"powercfg\", \"bcdedit\", \"diskpart\",\n    \"icacls\", \"attrib\", \"cacls\", \"takeown\", \"cipher\", \"compact\", \"defrag\",\n    \"chkdsk\", \"sfc\", \"dism\",\n];\n\nfn is_win32_cmd(cmd: &str) -> bool {\n    let clean = cmd.trim().to_ascii_lowercase();\n    let clean = clean.strip_suffix(\".exe\").unwrap_or(&clean);\n    WIN32_CMDS.contains(&clean)\n}\n\nfn translate_win_tokens(input: &str) -> String {\n    let mut s = replace_env_vars(input);\n    for (drive_pat, env_var) in &[\n        (\"C:\\\\\", \"${WSR_DRIVE_C}/\"),\n        (\"c:\\\\\", \"${WSR_DRIVE_C}/\"),\n        (\"C:/\", \"${WSR_DRIVE_C}/\"),\n        (\"c:/\", \"${WSR_DRIVE_C}/\"),\n        (\"D:\\\\\", \"${WSR_DRIVE_D}/\"),\n        (\"d:\\\\\", \"${WSR_DRIVE_D}/\"),\n        (\"D:/\", \"${WSR_DRIVE_D}/\"),\n        (\"d:/\", \"${WSR_DRIVE_D}/\"),\n        (\"Z:\\\\\", \"/\"),\n        (\"z:\\\\\", \"/\"),\n        (\"Z:/\", \"/\"),\n        (\"z:/\", \"/\"),\n    ] {\n        s = s.replace(drive_pat, env_var);\n    }\n    s.replace('\\\\', \"/\")\n}\n\nfn replace_env_vars(input: &str) -> String {\n    let mut out = String::with_capacity(input.len() + 16);\n    let chars: Vec<char> = input.chars().collect();\n    let mut i = 0;\n    while i < chars.len() {\n        if chars[i] == '%' {\n            if i + 2 < chars.len() && chars[i + 1] == '~' {\n                let mut j = i + 2;\n                while j < chars.len() && chars[j].is_ascii_alphabetic() {\n                    j += 1;\n                }\n                if j < chars.len() && chars[j].is_ascii_digit() {\n                    let mods: String = chars[i + 2..j].iter().collect();\n                    let digit = chars[j];\n                    if mods.eq_ignore_ascii_case(\"dp\") && digit == '0' {\n                        out.push_str(\"$(cd \\\"$(dirname \\\"$0\\\")\\\" && pwd)/\");\n                    } else {\n                        out.push('$');\n                        out.push(digit);\n                    }\n                    i = j + 1;\n                    continue;\n                }\n            }\n            if i + 1 < chars.len() && chars[i + 1] == '%' {\n                if i + 2 < chars.len() && chars[i + 2].is_ascii_alphanumeric() {\n                    out.push('$');\n                    out.push(chars[i + 2].to_ascii_lowercase());\n                    i += 3;\n                    continue;\n                }\n            }\n            if i + 1 < chars.len() && chars[i + 1] == '*' {\n                out.push_str(\"\\\"$@\\\"\");\n                i += 2;\n                continue;\n            }\n            if i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {\n                out.push('$');\n                out.push(chars[i + 1]);\n                i += 2;\n                continue;\n            }\n            if let Some(rel_end) = chars[i + 1..].iter().position(|&c| c == '%') {\n                let end = i + 1 + rel_end;\n                let var_name: String = chars[i + 1..end].iter().collect();\n                if !var_name.is_empty()\n                    && var_name\n                        .chars()\n                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '(' || c == ')')\n                {\n                    let mapped = match var_name.to_ascii_uppercase().as_str() {\n                        \"CD\" => \"PWD\".to_string(),\n                        \"RANDOM\" => \"RANDOM\".to_string(),\n                        \"ERRORLEVEL\" => \"?\".to_string(),\n                        \"USERPROFILE\" | \"HOMEPATH\" => \"USERPROFILE\".to_string(),\n                        \"SYSTEMDRIVE\" => \"SystemDrive\".to_string(),\n                        \"SYSTEMROOT\" | \"WINDIR\" => \"SystemRoot\".to_string(),\n                        \"PROGRAMFILES\" | \"PROGRAMFILES(X86)\" => \"ProgramFiles\".to_string(),\n                        \"PROGRAMDATA\" | \"ALLUSERSPROFILE\" => \"ProgramData\".to_string(),\n                        \"APPDATA\" => \"APPDATA\".to_string(),\n                        \"LOCALAPPDATA\" => \"LOCALAPPDATA\".to_string(),\n                        \"TEMP\" | \"TMP\" => \"TEMP\".to_string(),\n                        _ => var_name,\n                    };\n                    if mapped == \"?\" {\n                        out.push_str(\"$?\");\n                    } else {\n                        out.push_str(&format!(\"${{{}}}\", mapped));\n                    }\n                    i = end + 1;\n                    continue;\n                }\n            }\n            out.push('$');\n            i += 1;\n        } else {\n            out.push(chars[i]);\n            i += 1;\n        }\n    }\n    out\n}\n\nfn split_first_token(s: &str) -> Option<(&str, &str)> {\n    let s = s.trim();\n    if s.is_empty() {\n        return None;\n    }\n    if let Some(stripped) = s.strip_prefix('\"') {\n        if let Some(end_quote) = stripped.find('\"') {\n            let token = &s[..end_quote + 2];\n            let rest = s[end_quote + 2..].trim();\n            return Some((token, rest));\n        }\n    }\n    let mut iter = s.splitn(2, char::is_whitespace);\n    let first = iter.next()?;\n    let rest = iter.next().unwrap_or(\"\").trim();\n    Some((first, rest))\n}\n\nfn transpile_line(line: &str, skipped: &mut Vec<String>, open_func: &mut bool) -> String {\n    let trimmed = line.trim();\n    if trimmed.is_empty() {\n        return String::new();\n    }\n    if let Some(rest) = trimmed.strip_prefix(\"::\") {\n        return format!(\"# {}\", rest.trim());\n    }\n\n    let clean = trimmed.strip_prefix('@').unwrap_or(trimmed).trim();\n    let lower = clean.to_ascii_lowercase();\n\n    if clean.len() == 2 && clean.as_bytes()[1] == b':' && clean.as_bytes()[0].is_ascii_alphabetic() {\n        let drv = (clean.as_bytes()[0] as char).to_ascii_uppercase();\n        return match drv {\n            'C' => \"cd \\\"${WSR_DRIVE_C}\\\"\".to_string(),\n            'D' => \"cd \\\"${WSR_DRIVE_D}\\\"\".to_string(),\n            'Z' => \"cd /\".to_string(),\n            _ => \"cd \\\"${WSR_DRIVE_C}\\\"\".to_string(),\n        };\n    }\n\n    if lower == \"setlocal\" || lower.starts_with(\"setlocal \") || lower == \"endlocal\" || lower.starts_with(\"endlocal \") {\n        return format!(\"# [VFS: {}]\", clean);\n    }\n    if lower == \"echo off\" || lower.starts_with(\"echo off \") {\n        return \"set +x\".to_string();\n    }\n    if lower == \"echo on\" || lower.starts_with(\"echo on \") {\n        return \"set -x\".to_string();\n    }\n    if lower == \"echo.\" || lower == \"echo:\" {\n        return \"echo \\\"\\\"\".to_string();\n    }\n    if lower.starts_with(\"echo \") {\n        let msg = &clean[5..];\n        if let Some(redir_idx) = msg.find('>') {\n            let (text_part, redir_part) = msg.split_at(redir_idx);\n            let is_append = redir_part.starts_with(\">>\");\n            let target_raw = if is_append { &redir_part[2..] } else { &redir_part[1..] }.trim();\n            let target_conv = translate_win_tokens(target_raw.trim_matches('\"'));\n            let text_conv = translate_win_tokens(text_part.trim()).replace('\"', \"\\\\\\\"\");\n            let op = if is_append { \">>\" } else { \">\" };\n            return format!(\"echo \\\"{}\\\" {} \\\"{}\\\"\", text_conv, op, target_conv);\n        }\n        let converted = translate_win_tokens(msg).replace('\"', \"\\\\\\\"\");\n        return format!(\"echo \\\"{}\\\"\", converted);\n    }\n    if lower == \"rem\" || lower.starts_with(\"rem \") {\n        let comment = if clean.len() > 3 { clean[3..].trim() } else { \"\" };\n        return format!(\"# {}\", comment);\n    }\n    if lower == \"cls\" {\n        return \"clear\".to_string();\n    }\n    if lower == \"ver\" {\n        return \"echo \\\"Microsoft Windows [Version 10.0.22631.4317 - Android Rust VFS]\\\"\".to_string();\n    }\n    if lower == \"dir\" || lower.starts_with(\"dir \") {\n        let rest = if clean.len() > 3 { clean[3..].trim() } else { \"\" };\n        let arg = rest\n            .split_whitespace()\n            .filter(|t| !t.starts_with('/'))\n            .collect::<Vec<_>>()\n            .join(\" \");\n        let target = translate_win_tokens(&arg);\n        return if target.is_empty() {\n            \"ls -la\".to_string()\n        } else {\n            format!(\"ls -la {}\", target)\n        };\n    }\n    if lower == \"ipconfig\" || lower.starts_with(\"ipconfig \") {\n        return \"ip addr show 2>/dev/null || ifconfig 2>/dev/null || echo 'Active Network Interface: wlan0 (Android VFS)'\".to_string();\n    }\n    if lower == \"tasklist\" || lower.starts_with(\"tasklist \") {\n        return \"ps -ef 2>/dev/null || ps\".to_string();\n    }\n    if lower == \"systeminfo\" || lower.starts_with(\"systeminfo \") {\n        return \"echo \\\"OS Name: Microsoft Windows 11 Pro (Android Rust VFS)\\\" && uname -a\".to_string();\n    }\n    if lower.starts_with(\"where \") {\n        return format!(\"which {}\", translate_win_tokens(clean[6..].trim()));\n    }\n    if lower.starts_with(\"findstr \") {\n        let rest = clean[8..].trim();\n        let mut flags = String::from(\"-E\");\n        let mut tokens = Vec::new();\n        for tok in rest.split_whitespace() {\n            if tok.eq_ignore_ascii_case(\"/i\") {\n                flags.push_str(\" -i\");\n            } else if tok.eq_ignore_ascii_case(\"/n\") {\n                flags.push_str(\" -n\");\n            } else if !tok.starts_with('/') {\n                tokens.push(translate_win_tokens(tok));\n            }\n        }\n        return format!(\"grep {} {}\", flags, tokens.join(\" \"));\n    }\n    if lower == \"pause\" || lower.starts_with(\"pause \") {\n        return \"printf 'Press enter to continue...'; read -r _p\".to_string();\n    }\n    if lower == \"popd\" {\n        return \"popd\".to_string();\n    }\n    if lower.starts_with(\"pushd \") {\n        return format!(\"pushd {}\", translate_win_tokens(clean[6..].trim()));\n    }\n    if lower.starts_with(\"set \") {\n        let rest = clean[4..].trim();\n        let (is_arith, is_prompt, expr) = if rest.len() >= 2 && rest[..2].eq_ignore_ascii_case(\"/a\") {\n            (true, false, rest[2..].trim())\n        } else if rest.len() >= 2 && rest[..2].eq_ignore_ascii_case(\"/p\") {\n            (false, true, rest[2..].trim())\n        } else {\n            (false, false, rest)\n        };\n        if let Some(eq_idx) = expr.find('=') {\n            let var = expr[..eq_idx].trim();\n            let val = translate_win_tokens(expr[eq_idx + 1..].trim().trim_matches('\"'));\n            if is_arith {\n                return format!(\"export {}=$(( {} ))\", var, val);\n            } else if is_prompt {\n                return format!(\"printf '%s' \\\"{}\\\"; read -r {}; export {}\", val, var, var);\n            } else {\n                return format!(\"export {}=\\\"{}\\\"\", var, val);\n            }\n        }\n    }\n    if lower.starts_with(\"goto \") {\n        let label = clean[5..].trim().trim_start_matches(':').to_ascii_lowercase();\n        return if label == \"eof\" { \"exit 0\".to_string() } else { label };\n    }\n    if clean.starts_with(':') {\n        let label_name: String = clean[1..]\n            .chars()\n            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')\n            .collect();\n        if !label_name.is_empty() {\n            let prefix = if *open_func { \"}\\n\\n\" } else { \"\\n\" };\n            *open_func = true;\n            return format!(\"{}{}() {{\", prefix, label_name.to_ascii_lowercase());\n        }\n    }\n    if lower.starts_with(\"if not exist \") {\n        if let Some((p, cmd)) = split_first_token(clean[13..].trim()) {\n            let mut d = false;\n            return format!(\n                \"if [ ! -e \\\"{}\\\" ]; then {}; fi\",\n                translate_win_tokens(p.trim_matches('\"')),\n                transpile_line(cmd, skipped, &mut d)\n            );\n        }\n    }\n    if lower.starts_with(\"if exist \") {\n        if let Some((p, cmd)) = split_first_token(clean[9..].trim()) {\n            let mut d = false;\n            return format!(\n                \"if [ -e \\\"{}\\\" ]; then {}; fi\",\n                translate_win_tokens(p.trim_matches('\"')),\n                transpile_line(cmd, skipped, &mut d)\n            );\n        }\n    }\n    if lower.starts_with(\"if errorlevel \") {\n        if let Some((lvl, cmd)) = split_first_token(clean[14..].trim()) {\n            let mut d = false;\n            return format!(\"if [ $? -ge {} ]; then {}; fi\", lvl, transpile_line(cmd, skipped, &mut d));\n        }\n    }\n    if lower.starts_with(\"for \") {\n        if let Some(in_pos) = lower.find(\" in (\") {\n            let var_part = clean[4..in_pos].trim().trim_start_matches('%');\n            let var_name = var_part.chars().next().unwrap_or('i').to_ascii_lowercase();\n            let after_in = &clean[in_pos + 5..];\n            if let Some(close_paren) = after_in.find(')') {\n                let items = after_in[..close_paren].replace(',', \" \");\n                let after_paren = after_in[close_paren + 1..].trim();\n                let do_body = if after_paren.len() >= 3 && after_paren[..3].eq_ignore_ascii_case(\"do \") {\n                    after_paren[3..].trim()\n                } else {\n                    after_paren\n                };\n                let mut d = false;\n                return format!(\n                    \"for {} in {}; do\\n  {}\\ndone\",\n                    var_name,\n                    translate_win_tokens(&items),\n                    transpile_line(do_body, skipped, &mut d)\n                );\n            }\n        }\n    }\n    if lower.starts_with(\"call \") {\n        let t = translate_win_tokens(clean[5..].trim());\n        let t = if t.ends_with(\".bat\") || t.ends_with(\".cmd\") {\n            format!(\"{}.sh\", &t[..t.len() - 4])\n        } else {\n            t\n        };\n        return format!(\"sh {}\", t);\n    }\n    if lower == \"exit\" || lower.starts_with(\"exit \") {\n        let rest = clean[4..].trim();\n        let rest = if rest.len() >= 2 && rest[..2].eq_ignore_ascii_case(\"/b\") {\n            rest[2..].trim()\n        } else {\n            rest\n        };\n        return format!(\"exit {}\", if rest.is_empty() { \"0\" } else { rest });\n    }\n    if lower.starts_with(\"mkdir \") || lower.starts_with(\"md \") {\n        let idx = if lower.starts_with(\"mkdir \") { 6 } else { 3 };\n        return format!(\"mkdir -p {}\", translate_win_tokens(clean[idx..].trim()));\n    }\n    if lower.starts_with(\"rmdir \") || lower.starts_with(\"rd \") {\n        let idx = if lower.starts_with(\"rmdir \") { 6 } else { 3 };\n        let mut arg = clean[idx..].trim();\n        while arg.len() >= 2 && (arg[..2].eq_ignore_ascii_case(\"/s\") || arg[..2].eq_ignore_ascii_case(\"/q\")) {\n            arg = arg[2..].trim();\n        }\n        return format!(\"rm -rf {}\", translate_win_tokens(arg));\n    }\n    if lower.starts_with(\"copy \") || lower.starts_with(\"xcopy \") {\n        let idx = if lower.starts_with(\"xcopy \") { 6 } else { 5 };\n        return format!(\"cp -r {}\", translate_win_tokens(clean[idx..].trim()));\n    }\n    if lower.starts_with(\"move \") {\n        return format!(\"mv -f {}\", translate_win_tokens(clean[5..].trim()));\n    }\n    if lower.starts_with(\"del \") || lower.starts_with(\"erase \") {\n        let idx = if lower.starts_with(\"erase \") { 6 } else { 4 };\n        return format!(\"rm -f {}\", translate_win_tokens(clean[idx..].trim()));\n    }\n    if lower.starts_with(\"type \") {\n        return format!(\"cat {}\", translate_win_tokens(clean[5..].trim()));\n    }\n    if lower == \"cd\" || lower.starts_with(\"cd \") {\n        let mut rest = if clean.len() > 2 { clean[3..].trim() } else { \"\" };\n        if rest.len() >= 2 && rest[..2].eq_ignore_ascii_case(\"/d\") {\n            rest = rest[2..].trim();\n        }\n        let conv = translate_win_tokens(rest);\n        return if conv.is_empty() {\n            \"cd \\\"${USERPROFILE}\\\"\".to_string()\n        } else {\n            format!(\"cd {}\", conv)\n        };\n    }\n\n    let first_word = clean.split_whitespace().next().unwrap_or(\"\");\n    if first_word.to_ascii_lowercase().ends_with(\".exe\") && !is_win32_cmd(first_word) {\n        let exe_cmd = translate_win_tokens(clean);\n        return format!(\n            \"if command -v box64 >/dev/null 2>&1 && command -v wine64 >/dev/null 2>&1; then box64 wine64 {}; elif command -v wine >/dev/null 2>&1; then wine {}; else {}; fi\",\n            exe_cmd, exe_cmd, exe_cmd\n        );\n    }\n\n    if is_win32_cmd(first_word) {\n        skipped.push(trimmed.to_string());\n        return format!(\"# [SKIP Win32: {}]\", trimmed);\n    }\n\n    translate_win_tokens(clean)\n}\n\nfn main() {\n    let args: Vec<String> = env::args().collect();\n    if args.len() < 2 || args[1] == \"--version\" {\n        println!(\"bat2sh 3.0.0 (Rust Windows VFS + POSIX Shell Transpiler)\");\n        if args.len() < 2 {\n            eprintln!(\"Usage: bat2sh <path/to/script.bat>\");\n            process::exit(1);\n        }\n        return;\n    }\n\n    let input_path = Path::new(&args[1]);\n    let src = match fs::read_to_string(input_path) {\n        Ok(s) => s,\n        Err(e) => {\n            eprintln!(\"Failed to read {}: {}\", input_path.display(), e);\n            process::exit(1);\n        }\n    };\n\n    let home = env::var(\"HOME\").unwrap_or_else(|_| \"/data/data/com.termux/files/home\".to_string());\n    let drive_c = format!(\"{}/.wsr_vfs/drive_c\", home);\n    let _ = fs::create_dir_all(format!(\"{}/Windows/System32\", drive_c));\n    let _ = fs::create_dir_all(format!(\"{}/Program Files\", drive_c));\n    let _ = fs::create_dir_all(format!(\"{}/Users/Admin/Desktop\", drive_c));\n    let _ = fs::create_dir_all(format!(\"{}/Users/Admin/AppData/Local/Temp\", drive_c));\n\n    let preamble = format!(\n        \"export WSR_DRIVE_C=\\\"{c}\\\"\\n\\\n         export WSR_DRIVE_D=\\\"/sdcard\\\"\\n\\\n         export WSR_DRIVE_Z=\\\"/\\\"\\n\\\n         export SystemDrive=\\\"C:\\\"\\n\\\n         export SystemRoot=\\\"{c}/Windows\\\"\\n\\\n         export windir=\\\"{c}/Windows\\\"\\n\\\n         export ProgramFiles=\\\"{c}/Program Files\\\"\\n\\\n         export USERPROFILE=\\\"{c}/Users/Admin\\\"\\n\\\n         export TEMP=\\\"{c}/Users/Admin/AppData/Local/Temp\\\"\\n\\\n         export TMP=\\\"{c}/Users/Admin/AppData/Local/Temp\\\"\\n\",\n        c = drive_c\n    );\n\n    let mut out = vec![\n        \"#!/bin/sh\".to_string(),\n        \"# Transpiled by Rust bat2sh v3.0 (Windows VFS enabled)\".to_string(),\n        \"set -e\".to_string(),\n        preamble,\n    ];\n    let mut skipped = Vec::new();\n    let mut open_func = false;\n\n    for raw in src.lines() {\n        out.push(transpile_line(raw, &mut skipped, &mut open_func));\n    }\n    if open_func {\n        out.push(\"}\".to_string());\n    }\n    if !skipped.is_empty() {\n        out.push(String::new());\n        out.push(\"# === SKIPPED Windows-only lines ===\".to_string());\n        for s in &skipped {\n            out.push(format!(\"# {}\", s));\n        }\n    }\n\n    let out_path = input_path.with_extension(\"sh\");\n    let final_script = out.join(\"\\n\") + \"\\n\";\n    if let Err(e) = fs::write(&out_path, &final_script) {\n        eprintln!(\"Failed to write {}: {}\", out_path.display(), e);\n        process::exit(1);\n    }\n\n    println!(\"Written: {}\", out_path.display());\n    if !skipped.is_empty() {\n        println!(\"\\nSkipped {} Windows-only lines:\", skipped.len());\n        for s in &skipped {\n            println!(\"  # [SKIP Win32: {}]\", s);\n        }\n    }\n}\n";
}

function regParserRustSource() {
  return "#!/usr/bin/env rust-script\n//! Standalone zero-dependency Rust Windows `.reg` parser for AntiGem.\n//! Compile directly with: `rustc tools/reg_parser.rs -O -o ~/.antigem_tools/bin/reg_parser`\n\nuse std::env;\nuse std::fs;\nuse std::io::Write;\nuse std::path::{Path, PathBuf};\nuse std::process;\n\nfn read_reg_text(path: &Path) -> Result<String, String> {\n    let bytes = fs::read(path).map_err(|e| format!(\"Failed to read {}: {}\", path.display(), e))?;\n    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {\n        let u16_words: Vec<u16> = bytes[2..]\n            .chunks_exact(2)\n            .map(|c| u16::from_le_bytes([c[0], c[1]]))\n            .collect();\n        Ok(String::from_utf16_lossy(&u16_words))\n    } else {\n        Ok(String::from_utf8_lossy(&bytes).into_owned())\n    }\n}\n\nfn main() {\n    let args: Vec<String> = env::args().collect();\n    if args.len() < 2 || args[1] == \"--version\" {\n        println!(\"reg_parser 2.0.0 (Rust Registry Parser)\");\n        if args.len() < 2 {\n            eprintln!(\"Usage: reg_parser <path/to/file.reg>\");\n            process::exit(1);\n        }\n        return;\n    }\n\n    let reg_path = Path::new(&args[1]);\n    let content = match read_reg_text(reg_path) {\n        Ok(c) => c,\n        Err(e) => {\n            eprintln!(\"{}\", e);\n            process::exit(1);\n        }\n    };\n\n    let home_dir = env::var(\"HOME\").unwrap_or_else(|_| \"/data/data/com.termux/files/home\".to_string());\n    let bashrc_path = PathBuf::from(&home_dir).join(\".bashrc\");\n    let mut existing_bashrc = if bashrc_path.exists() {\n        fs::read_to_string(&bashrc_path).unwrap_or_default()\n    } else {\n        String::new()\n    };\n\n    let mut current_key: Option<String> = None;\n    let mut applied: Vec<String> = Vec::new();\n    let mut skipped: Vec<String> = Vec::new();\n\n    for raw in content.lines() {\n        let line = raw.trim().trim_start_matches('\\u{feff}');\n        if line.is_empty()\n            || line.starts_with(';')\n            || line.starts_with(\"Windows Registry Editor\")\n            || line.starts_with(\"REGEDIT4\")\n        {\n            continue;\n        }\n        if line.starts_with('[') && line.ends_with(']') {\n            current_key = Some(line[1..line.len() - 1].to_string());\n            continue;\n        }\n        if let Some(ref key) = current_key {\n            if let Some(eq_idx) = line.find('=') {\n                let n = line[..eq_idx].trim().trim_matches('\"');\n                let v = line[eq_idx + 1..].trim();\n                let key_low = key.to_ascii_lowercase();\n                let is_env = key_low.contains(\"hkcu\\\\environment\")\n                    || key_low.contains(\"hkey_current_user\\\\environment\")\n                    || key_low.contains(\"currentcontrolset\\\\control\\\\session manager\\\\environment\");\n                let v_low = v.to_ascii_lowercase();\n                let is_bin = v_low.starts_with(\"hex:\")\n                    || v_low.starts_with(\"dword:\")\n                    || v_low.starts_with(\"qword:\")\n                    || v_low.starts_with(\"hex(\");\n\n                if is_env && !is_bin && n != \"@\" {\n                    let val = v\n                        .trim_matches('\"')\n                        .replace(\"\\\\\\\\\", \"/\")\n                        .replace(\"%SystemRoot%\", \"/system\")\n                        .replace(\"%USERPROFILE%\", &home_dir)\n                        .replace(\"%TEMP%\", \"/tmp\")\n                        .replace(\"%TMP%\", \"/tmp\");\n                    let bash_line = format!(\"export {}=\\\"{}\\\"\", n, val);\n                    if !existing_bashrc.contains(&bash_line) {\n                        if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&bashrc_path) {\n                            let _ = writeln!(f, \"\\n{}\", bash_line);\n                            existing_bashrc.push('\\n');\n                            existing_bashrc.push_str(&bash_line);\n                        }\n                    }\n                    applied.push(format!(\"  {} = {}\", n, val));\n                } else {\n                    skipped.push(format!(\"  [{}] {} (Windows-only or binary)\", key, n));\n                }\n            }\n        }\n    }\n\n    println!(\"=== .reg Parse Report ===\");\n    println!(\"File: {}\", reg_path.display());\n    println!(\"\\nApplied to ~/.bashrc ({}):\", applied.len());\n    if applied.is_empty() {\n        println!(\"  (none)\");\n    } else {\n        for a in &applied {\n            println!(\"{}\", a);\n        }\n    }\n    println!(\"\\nSkipped - Windows-only ({}):\", skipped.len());\n    if skipped.is_empty() {\n        println!(\"  (none)\");\n    } else {\n        for s in skipped.iter().take(10) {\n            println!(\"{}\", s);\n        }\n        if skipped.len() > 10 {\n            println!(\"  ...and {} more\", skipped.len() - 10);\n        }\n    }\n    println!(\"\\nRun: source ~/.bashrc  to reload env vars\");\n}\n";
}

async function handleTool(name, args) {
  switch (name) {
    case 'wsr_list_directory': {
      const p = translateWinPath(args.path || 'C:\\Users\\Admin\\Desktop');
      return {
        command: `mkdir -p "$HOME/.wsr_vfs/drive_c/Users/Admin/Desktop" "$HOME/.wsr_vfs/drive_c/Windows/System32" "$HOME/.wsr_vfs/drive_c/Program Files" && ls -la "${p}" 2>&1`,
        note: 'Run this command in the terminal MCP to list the Windows VFS directory.'
      };
    }

    case 'wsr_run_script': {
      const strategy = detectStrategy(args.file_path);
      if (!strategy) {
        return { error: `Unsupported file type: ${args.file_path}. Supported: .bat .cmd .exe .msi .ps1 .vbs .reg .rs .py .js .sh` };
      }
      return {
        strategy,
        command: buildCommand(args.file_path, strategy, args.args || ''),
        note: 'Run this command in the terminal MCP.'
      };
    }

    case 'wsr_inspect_exe': {
      const p = translateWinPath(args.file_path || '');
      return {
        command: `~/.antigem_tools/bin/wsr pe "${p}" 2>/dev/null || file "${p}"`,
        note: 'Inspects PE header metadata for .exe/.dll files.'
      };
    }

    case 'wsr_read_script': {
      const p = translateWinPath(args.file_path || '');
      return {
        command: `cat "${p}" 2>&1`,
        note: 'Run in terminal MCP to read the file.'
      };
    }

    case 'wsr_transpile_bat': {
      const p = translateWinPath(args.file_path || '');
      return {
        command: `~/.antigem_tools/bin/bat2sh "${p}" 2>&1`,
        post_process: 'Runs the native Rust bat2sh binary to transpile the .bat/.cmd file with Windows VFS drive mappings.',
        note: 'If bat2sh is not yet compiled, run wsr_setup_tools first.'
      };
    }

    case 'wsr_setup_tools': {
      return {
        commands: [
          'mkdir -p ~/.wsr_vfs/drive_c/Windows/System32 ~/.wsr_vfs/drive_c/Users/Admin/Desktop ~/.wsr_vfs/drive_c/Temp',
          'mkdir -p ~/.antigem_tools/src ~/.antigem_tools/bin',
          `cat > ~/.antigem_tools/src/bat2sh.rs << 'RSEOF'\n${bat2shRustSource()}\nRSEOF`,
          `cat > ~/.antigem_tools/src/reg_parser.rs << 'RSEOF'\n${regParserRustSource()}\nRSEOF`,
          'pkg install rust -y 2>/dev/null || true',
          'rustc ~/.antigem_tools/src/bat2sh.rs -O -o ~/.antigem_tools/bin/bat2sh',
          'rustc ~/.antigem_tools/src/reg_parser.rs -O -o ~/.antigem_tools/bin/reg_parser',
          'chmod +x ~/.antigem_tools/bin/bat2sh ~/.antigem_tools/bin/reg_parser',
          'rustc --version && ~/.antigem_tools/bin/bat2sh --version && ~/.antigem_tools/bin/reg_parser --version'
        ],
        note: 'Run each command in the terminal MCP in order to initialize Windows VFS and compile the Rust helper binaries.'
      };
    }

    case 'wsr_check_environment': {
      return {
        command: [
          'echo "=== Rust & Windows VFS Environment Check ==="',
          'echo "Drive C: -> $HOME/.wsr_vfs/drive_c"',
          'echo "Drive D: -> /sdcard"',
          'echo "Drive Z: -> /"',
          'which rustc && rustc --version || echo "rustc: missing (pkg install rust)"',
          'which box64 2>/dev/null && box64 --version || echo "box64: optional (for x86_64 .exe)"',
          'which wine 2>/dev/null && wine --version || echo "wine: optional (for Win32 .exe/.vbs)"',
          'which pwsh 2>/dev/null && pwsh --version || echo "pwsh: optional (pkg install powershell)"',
          'ls -la ~/.antigem_tools/bin/ 2>/dev/null || echo "Rust helper binaries not compiled (use wsr_setup_tools)"'
        ].join(' && '),
        note: 'Run in terminal MCP.'
      };
    }

    default:
      return { error: `Unknown tool: ${name}` };
  }
}

if (typeof self !== 'undefined' && typeof self.addEventListener === 'function') {
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

if (typeof chrome !== 'undefined' && chrome.runtime?.onMessage) {
  chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
    const { type, tool, args } = message;
    if (type === 'MCP_TOOLS_LIST') {
      sendResponse({ tools: MCP_TOOLS });
      return true;
    }
    if (type === 'MCP_TOOL_CALL') {
      handleTool(tool, args || {}).then(result => sendResponse({ result }));
      return true;
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

if (typeof module !== 'undefined') {
  module.exports = {
    MCP_TOOLS,
    handleTool,
    transpileBat,
    translateWinPath,
    detectStrategy,
    bat2shRustSource,
    regParserRustSource
  };
}
