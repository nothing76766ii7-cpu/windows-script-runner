# Windows Script Runner — Full Build Instructions
# Written for an AI agent to read, understand, and complete

---

## WHAT THIS PLUGIN IS

This is a plugin for an Android app called AntiGem (https://github.com/santoshkurmi/antigem).

AntiGem is a mobile IDE for Android that includes:
- A code editor
- A terminal (powered by Termux, which runs a full Linux environment)
- An AI chat interface
- A plugin system called "Custom AntiGem Plugins"

This plugin adds the ability to run Windows script files on Android.
Since Android runs Linux (not Windows), it cannot run .bat or .ps1 files natively.
This plugin solves that by either transpiling them (converting to Linux shell commands)
or running them through compatibility layers like Wine or PowerShell for Linux.

---

## WHAT FILES EXIST IN THIS ZIP

Here is every file and what it does:

### manifest.json
Tells AntiGem who this plugin is. Required.
- id: unique identifier for the plugin
- name: display name shown in AntiGem
- version: version number
- description: short description shown in AntiGem
- worker: points to the background JavaScript file (worker.js)
- ui: points to the UI panel HTML file (ui/index.html)

### worker.js
The background JavaScript that runs inside AntiGem's plugin system.
It does three things:
1. Defines MCP tools — these are functions that Claude (the AI inside AntiGem) can call
2. Contains the bat-to-sh transpiler logic written in JavaScript
3. Bridges messages between the UI panel and AntiGem's terminal

The MCP tools it registers are:
- wsr_list_directory: lists files in a folder on the phone
- wsr_run_script: builds the correct command to run a script file
- wsr_read_script: reads the contents of a script file
- wsr_transpile_bat: converts a .bat file to a .sh file without running it
- wsr_setup_tools: installs the Python helper scripts onto the phone
- wsr_check_environment: checks what execution tools are installed (python3, pwsh, wine, etc.)

### ui/index.html
The visual interface. It looks like Windows File Explorer.
It has:
- A title bar at the top (Windows-style with minimize/maximize/close buttons)
- A toolbar with buttons: Up, Refresh, Run, Edit, Copy Path, New Folder, Setup, Check Env, Help
- An address bar with clickable breadcrumb navigation and a search box
- A left sidebar with quick-access folders and script type filters
- A main file list showing files with icons, size, type, modified date
- A terminal panel at the bottom where commands run and output appears
- A right-click context menu for files
- A run dialog that appears when you double-click a script file

The file list shows special RUN badges on executable scripts (.bat, .ps1, .vbs, .reg, .py, .js, .sh).
Double-clicking a runnable script opens a dialog showing the command that will run, with an optional arguments field.
Double-clicking a folder navigates into it.
Double-clicking a non-runnable file shows its contents in the terminal.

The terminal at the bottom accepts typed commands. It understands:
- cd <path>: navigates the file browser to that path
- ls or dir: refreshes the file list
- clear or cls: clears terminal output
- Any other command: runs it in AntiGem's terminal

The UI communicates with the background worker via two paths:
1. chrome.runtime.sendMessage (the AntiGem plugin API)
2. window.antigem.terminal.run (the AntiGem window bridge API)
If neither is available (opening in a plain browser for testing), it shows a preview of what would run.

### skills/run_windows_script.md
A guide for the AI agent inside AntiGem.
When a user asks Claude to run a Windows script, Claude reads this file to know what to do.
It maps each file extension to a strategy and sub-skill.
It tells Claude to attempt execution first and report any Windows-only skipped lines after.

### skills/bat_transpiler.md
A guide for the AI agent about how .bat and .cmd files get converted.
Contains the full mapping table of every Windows batch command and its Linux shell equivalent.
Lists which Windows commands have no equivalent and get skipped.
Warns about Termux's sh being mksh/toybox, not full bash.

### skills/ps1_android.md
A guide for the AI agent about running PowerShell scripts.
Three options in order:
1. pkg install powershell (easiest, may not be available)
2. Download PowerShell ARM64 binary from GitHub
3. Manually transpile simple scripts to bash

Lists PowerShell cmdlets that have no Android equivalent.

### skills/vbs_wine.md
A guide for the AI agent about running VBScript files.
Uses proot-distro to run Ubuntu inside Termux, then Wine inside Ubuntu, then wscript.exe inside Wine.
Explains the difference between wscript (GUI) and cscript (console, recommended).
Lists which COM objects and ActiveX things will fail.

### skills/reg_handler.md
A guide for the AI agent about parsing Windows .reg files.
Explains which registry keys can be mapped to Linux (environment variables → ~/.bashrc)
and which are Windows-only and just get reported.

### antigem-plugins.json
Used if this plugin is hosted on GitHub as a plugin repository.
AntiGem can point to a GitHub repo and read this file to find available plugins.
If installing by ZIP, this file is not strictly needed but good to have.

---

## WHAT FILES ARE MISSING / WHAT THE AI AGENT NEEDS TO CREATE

The following Python scripts are NOT included as separate files in this ZIP.
They are embedded as strings inside worker.js and get written to the phone
when the user clicks the Setup button in the toolbar.

However, if you want them as standalone files in the ZIP for clarity, create:

### tools/bat2sh.py
A Python 3 script that takes a .bat or .cmd file as input and writes a .sh file.
Run it like this: python3 bat2sh.py /path/to/script.bat
It reads the bat file, applies the mapping table from bat_transpiler.md,
writes the result to the same path with a .sh extension,
and prints a summary of any Windows-only lines it skipped.
The script is already embedded as a string in worker.js inside the getBat2shScript() function
and in worker.js inside the bat2shPython() function. Copy that string to make the standalone file.

### tools/reg_parser.py
A Python 3 script that takes a .reg file as input.
Run it like this: python3 reg_parser.py /path/to/file.reg
It reads the registry file, finds HKCU\Environment keys,
writes them as export VAR="value" lines to ~/.bashrc,
and prints a full report of what it applied and what it skipped.
The script is already embedded in worker.js inside the regParserPython() function.
Copy that string to make the standalone file.

---

## HOW ANTIGEM PLUGINS WORK

AntiGem plugins are folders (or ZIP files) with this structure:

```
plugin-name/
├── manifest.json        required — plugin metadata
├── worker.js            optional — background MCP tool worker
├── ui/
│   └── index.html       optional — WebView panel shown in AntiGem
└── skills/
    └── *.md             optional — guides that teach Claude how to use the plugin
```

The plugin gets loaded by AntiGem when:
- Installed as a ZIP via Settings → Custom AntiGem Plugins → Install from ZIP
- Or pulled from a GitHub repo added in Settings → Repositories

When loaded:
- worker.js runs as a background service worker
- ui/index.html is shown as a panel in AntiGem
- skills/*.md files are read by Claude when it needs to perform tasks from this plugin
- The MCP tools defined in worker.js become available to Claude via the Custom Plugin MCP

The API for worker.js uses either:
- chrome.runtime.onMessage (Chrome extension style, older AntiGem API)
- self.addEventListener('message', ...) (Service worker style, newer AntiGem API)
- module.exports (direct export if AntiGem loads it as a Node module)
worker.js in this plugin handles all three paths.

---

## HOW SCRIPT EXECUTION WORKS

When a user double-clicks a .bat file in the UI:

1. The UI shows a dialog with the file path and the command that will run
2. The command is built by the buildCmd() function in ui/index.html
3. For .bat files, the command is:
   python3 ~/.antigem_tools/bat2sh.py "/path/to/script.bat" && sh "/path/to/script.sh"
4. The command goes through exec() in ui/index.html
5. exec() sends it to the worker via chrome.runtime.sendMessage({ type: 'RUN_COMMAND', cmd })
6. The worker passes it to AntiGem's terminal MCP via self.antigem.terminal.run(cmd)
7. Output streams back line by line to the terminal panel

For .ps1 files: pwsh -File "/path/to/script.ps1"
For .vbs files: proot-distro login ubuntu -- wine wscript.exe "/path/to/script.vbs"
For .reg files: python3 ~/.antigem_tools/reg_parser.py "/path/to/file.reg"
For .py files:  python3 "/path/to/script.py"
For .js files:  node "/path/to/script.js"
For .sh files:  chmod +x "/path/to/script.sh" && sh "/path/to/script.sh"

---

## HOW TO INSTALL THIS PLUGIN

Option 1 — ZIP install:
1. Take the windows-script-runner folder and ZIP it
2. Open AntiGem on Android
3. Go to Settings → Custom AntiGem Plugins → Install from ZIP
4. Select the ZIP file
5. The plugin appears in AntiGem

Option 2 — GitHub repo:
1. Push this folder to a public GitHub repository
2. Make sure antigem-plugins.json is at the root of the repo
3. Open AntiGem → Settings → Repositories → Add repository
4. Enter the GitHub username/reponame
5. Find the plugin in the list and install it

After installing:
1. Open the plugin's UI panel in AntiGem
2. Click the Setup button in the toolbar to install the Python helper scripts
3. Click Check Env to verify what execution tools are available
4. Navigate to a folder containing script files
5. Double-click any script to run it

---

## HOW TO TEST THE UI WITHOUT ANTIGEM

Open ui/index.html in any desktop browser.
It will show the full Windows Explorer interface with mock demo files.
Execution will show [PREVIEW] messages instead of actually running commands
because the AntiGem bridge (chrome.runtime / window.antigem) is not available.
This is correct behavior — it means the file is working properly.

---

## WHAT THE AI AGENT SHOULD DO WITH THIS

If you are an AI agent reading this file, here is your task list:

1. Read every file in this ZIP to understand the full plugin.

2. Create tools/bat2sh.py as a standalone Python file.
   The source is inside worker.js in the bat2shPython() function return value.
   Copy the string content, unescape it properly, and save as tools/bat2sh.py.

3. Create tools/reg_parser.py as a standalone Python file.
   The source is inside worker.js in the regParserPython() function return value.
   Same process — copy, unescape, save as tools/reg_parser.py.

4. Test that ui/index.html opens in a browser without JavaScript errors.

5. If the AntiGem plugin API has changed from what is in worker.js,
   update the message handler at the bottom of worker.js to match the current API.
   The API documentation is in AntiGem's settings or at:
   https://github.com/santoshkurmi/antigem

6. If the plugin system now supports a different manifest format,
   update manifest.json to match.

7. Repackage everything as a ZIP file named windows-script-runner.zip

8. The ZIP should contain one top-level folder named windows-script-runner/
   with all files inside it. AntiGem expects this structure.

---

## KNOWN LIMITATIONS TO COMMUNICATE TO THE USER

- .bat files that call Windows-specific commands (reg, net, ipconfig, tasklist, wmic, etc.)
  will have those lines skipped. The rest runs fine.

- .ps1 files need PowerShell Core ARM64 installed. First-time setup downloads ~70MB.

- .vbs files need proot-distro + Ubuntu + Wine. First-time setup downloads ~300-500MB
  and takes several minutes. After setup, runs fast.

- .exe and .msi files cannot run. Wine may run simple .exe files but it is unreliable
  and not attempted automatically.

- .reg files cannot apply Windows registry settings. Only environment variables
  (HKCU\Environment) get written to ~/.bashrc. Everything else is reported only.

- Some scripts that call Windows APIs (COM, ActiveX, WMI, Win32) will partially fail.
  The plugin attempts execution and reports what failed — it does not block.

---

## FILE ENCODING AND FORMATTING NOTES

- All files are UTF-8
- Line endings are LF (Unix), not CRLF (Windows)
- JSON files have no trailing commas
- JavaScript uses single quotes for strings where possible
- The HTML file is a single self-contained file with all CSS and JS inline
- No external CDN dependencies in any file — everything works offline
- The skills/*.md files use standard Markdown with fenced code blocks

---

END OF INSTRUCTIONS
