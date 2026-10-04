# misiuscode — design spec

**Date:** 2026-10-04 · **Status:** approved design, awaiting implementation plan
**Figma (visual reference, dark IDE):** https://www.figma.com/design/6NH86hQSCyedyUvuUtuiCT — top frame `2:22` ("Agentic App – Dark IDE") on page `0:1`.

## What it is

A lightweight GTK4 desktop wrapper around the **raw Claude Code CLI**. It is one window that hosts, for one project directory at a time: a code editor, a file explorer, an agent tab running `claude`, a plain terminal, and Run output. The agent tab is *not* a chat wrapper: it is a real terminal running the unmodified `claude` binary, so the user's existing CLI setup (including Ollama routing configured at the CLI/environment level) works with zero integration code.

**Out of scope (v1):** Problems tab (per user decision), chat-style agent input box / paperclip / send from the Figma design, Edit and Window and Help menus, git status beyond the branch name, symbol breadcrumbs, panel-collapse toggle, settings UI, desktop file, packaging.

## Stack (decided)

- **GTK4 + GtkSourceView 5 + VTE 4, bound via Python (PyGObject).**
- Rationale: VTE is the terminal emulator widget GNOME Terminal itself uses, so the interactive `claude` TUI renders correctly (ANSI, redraws, prompts); GtkSourceView provides the editor with highlighting; idle RAM ~70–110MB. Python was chosen over the identical-widget Rust binding for fastest iteration and easiest user maintenance; the widget layer is identical if it ever gets ported.
- Runtime install (Arch/CachyOS): `pacman -S gtk4 gtksourceview5 vte4 python-gobject`. No pip packages.
- Implementation-session note: confirm exact PyGObject signatures by introspecting the installed bindings (`python3 -c "from gi.repository import Vte; print(dir(Vte.Terminal))"`) before relying on a specific spawn method; VTE's spawn API has legacy `spawn_sync` and newer `spawn_async` variants.

## Assumptions

- "Supports Ollama" = inherited automatically: the wrapper spawns `claude` unchanged with the user's environment, and the user's `claude` is already configured to route through Ollama. Nothing in the app configures models/providers.
- One project at a time: opening a different folder replaces the whole workspace (children killed, tabs cleared, tree rebuilt).
- Launcher use case is local: user starts it from a terminal or script (`bin/misiuscode [path]`), not from .desktop.

## Layout

```
HeaderBar (native GTK window titlebar)
  [File ▾] ········spacer······ [▶ Run] [■ Stop]
┌──────────────────────────────────────────┬──────────────┐
│ editor GtkPaned(vertical)                │ File Browser │
│  ├ Notebook: file tabs                   │  search box  │
│  │    (each tab: dirty-dot, ✕)           │  WORKSPACE   │
│  └ bottom panel Notebook:                │  lazy tree   │
│     Agent Console | Output | Terminal    │  ──────────  │
│     (status dot + trash in tab strip)    │  `main` · 12 │
└──────────────────────────────────────────┴──────────────┘
```

- HeaderBar holds the File `GtkPopoverMenuButton` on the left and Run/Stop as flat buttons on the right (matching the Figma "Run Controls" cluster `2:29` at the top right).
- Main split: `GtkPaned` (horizontal), right pane default width 395px (Figma `3:61`); left pane is an inner `GtkPaned` (vertical): editor default height 594px vs console 324px (Figma `2:38`/`3:3`).
- Window default 1586×992, resizable; dark by default. Theme is not system-following in v1.

## Theming

CSS overrides via `Gtk.StyleContext.add_provider_for_display`. Palette sampled from the Figma render:

| token | hex | use |
|---|---|---|
| `bg-window` | `#07090d` | window/menubar background |
| `surface-1` | `#0a0d13` | tab strips, search box, header buttons |
| `surface-2` | `#0d1017` | editor area, file browser body |
| `surface-3` | `#141720` | console output area |
| `panel-active` | `#111723` | active tab background |
| `accent` | `#2f80ed` | active-tab indicator, selection accents, Run |
| `selection` | `#1b3560` | selected tree row |
| `ok` | `#22c55e` | Run button / "Running" dot |
| `err` | `#ef4444` | Stop button / exit-dot |

UI text is light gray (`~#cbd5e1`), muted gray for labels (`~#6b7282`); editor colors come from a bundled GtkSource style scheme file written to these tokens (dark scheme based on `Adwaita-dark` palette, tuned to match the Figma code view).

## Components

Single Python package, ~8 files, no framework — plain callbacks between components.

```
bin/misiuscode            # shebang launcher: sys.path fix + glib mainloop; optional CLI arg = project path
misiuscode/
  main.py                 # Gtk.Application, loads CSS + style scheme, builds state.json dirs
  window.py               # headerbar, paned layout, wires components, File menu actions
  editor.py               # Notebook + GtkSourceView tabs (open/save/dirty/close, breadcrumb label)
  filetree.py             # right panel: search box, lazy tree, monitors, status row
  panels.py               # bottom Notebook: AgentConsole / Output / Terminal VTE widgets
  runctl.py               # project-type detection + Run/Stop → output pane
  gitstatus.py            # branch label reader (.git/HEAD via Gio.FileMonitor)
  state.py                # state.json load/save (recents, last project, per-project open tabs)
tests/test_selfcheck.py   # plain asserts, no pytest
```

### editor.py

- `GtkSource.View` + `GtkSource.Buffer` per file; language auto-detected via `GtkSource.LanguageManager`; bundled dark `StyleScheme` applied; line numbers on; font `Monospace 11`.
- Tabs are `Gtk.Notebook` pages with a custom tab-header widget (file name, dirty dot visible when `buffer.modified`, close `✕`). Active-tab styling: `panel-active` bg + accent underline (CSS).
- Ctrl+S saves asynchronously (`GtkSource.Buffer.save_async` or Gio write of the buffer to disk); dirty dot clears. Closing a modified tab raises a `Gtk.MessageDialog` ("Discard changes?" / Cancel).
- Breadcrumb row above the buffer shows only the relative path from project root (Figma also shows symbols `App` — dropped, v1 is paths only).
- If file load fails, show an error dialog and do not create the tab. If an open file is deleted on disk (from a monitor event), show a "file was deleted — close tab?" dialog.
- No tab-wide save-on-focus-loss, no autosave (v1).

### panels.py — three VTE tabs

All tabs spawn children in the project root with an env that is `os.environ` plus `~/.local/bin` appended to `PATH` if missing (GUI-launched processes often lack that path, and that is where `claude` lives for this user).

- **Agent Console (default tab):** spawns raw `claude`. Spawn is deferred until the tab is first displayed (don't launch three children at app startup). Shows the claude TUI directly.
- **Terminal:** spawns `$SHELL` (fallback `/bin/bash`) in the project root.
- **Output:** spawns the run command (from runctl). Trash button = `set_scrollback_lines(0)` / clear; status dot in the tab strip reflects Running (green pulsing not needed — static green), Dead/"Exit N" (red), Ready.
- On project switch: `SIGHUP → SIGKILL (grace 2s)` to every spawned child's pty/session, all panes reset, Agent Console/Terminal respawn lazily in the new root on next activation.

### runctl.py

Detection on project open, stored on the workspace state:

1. `<root>/package.json` exists → load JSON lazily; if `scripts.dev` → command `["npm", "run", "dev"]`; else if `scripts.start` → `["npm", "start"]`; else none.
2. else `*.csproj` in root → `["dotnet", "run"]`; else `.csproj` one level down (`*/*.csproj`) → `["dotnet", "run", "--project", dir]` (more than one match → alphabetically first). else none.
3. none → Run button disabled (tooltip "No dev/start script or .csproj found").

Run button triggers Output pane spawn (auto-switching to the Output tab). Stop = `os.killpg(os.getpgid(child_pid), SIGKILL)` wrapped in try/except `ProcessLookupError` — a VTE pty child is a session leader, so npm's child node processes share the pgid and die with it. Clicking Run while running: ignored (Stop first).

### filetree.py

- Tree widget: `Gtk.TreeView` + `Gtk.TreeStore`, lazily populated on expand (children scanned only when first expanded), sorted dirs-then-files, dotfiles visible except skip-list.
- Skip on scan: `node_modules`, `.git`, `obj`, `bin`, `vendor`, `__pycache__` (plus the editor's open project root itself).
- `Gio.FileMonitor` on each expanded directory (cap 50 monitors; `ponytail:` ceiling — unexpanded dirs won't live-update, that's fine) to add/remove entries; new directories populate lazily like everywhere else.
- Search box (`Gtk.SearchEntry`, 200ms debounce): flat scandir walk of the project (skip-list honored), filename-substring match, replaces tree content with a flat list of matching paths while non-empty.
- Status row: `branch` from `gitstatus.py` + total file count (excluding skip-list, computed on open).

### gitstatus.py

- Branch = parse `<root>/.git/HEAD` (`ref: refs/heads/<branch>`). Subscribe a `Gio.FileMonitor` on `.git/HEAD` (covers checkout/branch switches) and re-read also on window focus.
- Not a git repo → hide the branch text entirely. No other git information is shown in v1.

### state.py — `~/.config/misiuscode/state.json`

```json
{
  "last_project": "/home/…/someproj",
  "recents": ["/home/…/someproj"],
  "projects": {
    "/home/…/someproj": { "open_tabs": ["src/main.tsx"], "active_tab": 0 }
  }
}
```

- Startup with no CLI arg → open `last_project` if it still exists, else show an empty workspace with a disabled Run and empty tree (the user then uses File ▸ Open).
- Recents: most-recent-first, max 10, de-duplicated. `open_tabs`/`active_tab` saved on close/quit or when tabs change (debounced 1s write).

## Menus

Only **File** is built (Edit/Window/Help dropped — the editor has native undo/redo/Ctrl+S and the design's other menus would be stubs):

- **Open Project…** — `Gtk.FileChooserNative` for a folder.
- **Open Recent ▸** — up to 10 absolute paths from state.json.
- **New Project…** — dialog: pick parent dir + project name → `mkdir` + `git init` (both via subprocess, feedback on failure) → open as workspace. No scaffolding.
- **Quit** (Ctrl+Q).

## Error handling summary

| Failure | Behavior |
|---|---|
| `claude`/`dotnet`/`npm` binary missing at spawn | VTE prints its own exec error inside the pane; nothing crashes |
| No detected run command | Run disabled + tooltip |
| File open/save failure | dialog; tab not created / dirty dot stays |
| Tree/search monitor errors | logged to stderr, UI continues |
| Bad/missing `scripts` JSON | treated as "no dev command" (Run → `npm start` fallback path skipped cleanly) |

## Testing

One file, `tests/test_selfcheck.py`, plain asserts + `python3 tests/test_selfcheck.py` exits 0:

1. **Detection:** fixture dirs in `tests/fixtures/` — npm-with-dev, npm-start-only, package.json-without-scripts, csproj-in-root, csproj-nested, empty → assert exact argv (including `--project` form) or `None`.
2. **Git branch parsing:** `ref: refs/heads/main` → `main`; detached HEAD SHA content → hidden/disabled behavior; missing `.git` → hidden.
3. **state.json roundtrip:** write/load with the schema above, recents cap at 10 and dedupe.

UI itself is verified by hand in the implementation plan (walkthrough checklist per component), not unit-tested.

## Handoff notes for the implementing session

- Read this file first; then, before writing any VTE code, introspect the installed binding to confirm `Vte.Terminal.spawn_async(...)` signature (kwargs differ between VTE versions) and `GtkSource` scheme loading.
- Figma styling: if pixel-accurate tab/header spacing is required, call `get_design_context` on frame `2:22` from the Figma MCP; the palette table above is the fallback.
- After implementation, this project itself is not yet a git repo — end with instructions to the user to run:
  ```bash
  cd ~/FunProjects/misiuscode
  git init && git add -A && git commit -m "misiuscode v1"
  ```
- Known v1 ceilings (ponytail comments live in code): 50-monitor cap in the tree, no live update for unexpanded dirs, no autosave, no git status beyond branch.