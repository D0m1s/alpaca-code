# Architecture — alpaca-code v1

Single-process GTK4 app. No IPC, no daemons; the only external things it drives
are `claude`/`$SHELL`/`npm`/`dotnet` as VTE child processes and git via files it reads.

## Window layout

```
ApplicationWindow  (titlebar: HeaderBar [File ▾ | ▶ Run  ■ Stop])
└─ HPane (default 1181)
   ├─ VPane (default 594)
   │  ├─ Editor    (tabbed GtkSourceView)
   │  └─ Panes     (status row + Notebook: Agent Console | Output | Terminal, VTE)
   └─ FileBrowser  (search, lazy TreeView, status row: branch | file count)
```

## Module graph

```
bin/alpaca-code → alpaca_code.main.run
main: _theme_setup() (dark + CSS + scheme path) BEFORE window construction —
      the editor resolves its scheme in __init__
window.Window(root): builds Editor/Panes/FileBrowser; if root → set_workspace()
window.set_workspace → dirty gate (dialog) → _set_workspace_now:
      persist recents/last_project → tree.set_root → panes.set_root →
      Run button detection → editor.restore(project_tabs(state))
editor.on_state_changed → window.persist_tabs (1s delayed write)
panes.on_status → window._on_run_status (Run/Stop button sensitivity)
```

## VTE and processes (panels.py)

- `spawn(term, cwd, argv, on_ready=None)`: `Vte.Terminal.spawn_async`, **keyword
  form only** on this build (Vte-3.91), flags `SEARCH_PATH | SEARCH_PATH_FROM_ENVP`,
  env = full environ with `~/.local/bin` appended to PATH (so bare `claude` resolves).
- Agent Console runs `claude`, Terminal runs `$SHELL`, Output runs the run command.
- Children are **session leaders** → `_kill_tree`: `killpg(SIGHUP)`, then SIGKILL
  escalation after 2s. npm/node grandchildren die with the group.
- **Run lifecycle**: `launch_run` → `_run_starting = True` (pid not known yet) →
  `landed()` callback → pid tracked in `_pids["output"]` + status "Running". Spawn
  error → "Run failed to start: …"; instant death → "Process ended (label)".
  `has_running_run()` is true through the whole starting window (double-Run guard);
  Stop during it sets `_run_stop_pending` and kills the pid on landing.
  `_run_child_exited` maps raw wait status → "Exit N" (green when 0) and ignores
  cleanup exits so kills don't fake statuses.
- **Pane lifecycle**: user exits claude/shell → auto-respawn with a budget of 3
  consecutive revives; a child that lived ≥60s re-arms the budget; re-entering the
  tab re-arms it; a child dying within 3s of spawn is a broken child (no revival,
  no loop). `set_root` kills live children, pre-counts their exit events in
  `_kills_pending`, resets everything, and respawns the visible pane lazily.

## File watching

- `filetree.py`: one directory monitor per expanded dir (cap 50) — created/deleted/
  moved refills that row and prunes stale recorded expansions of its subtree;
  `.git/HEAD` monitor drives the branch label ("main" ↔ "" on detached HEAD);
  any store rebuild (set_root, search-clear) resets `_expanded`.
- `editor.py`: one file monitor per open file — DELETED → "File was deleted —
  close its tab?".

## Persistence (state.py)

Written atomically (tmp + `os.replace`) on every workspace open and on editor
state changes (1s delayed). **Invariant: reads never raise** — `load()` and
`project_tabs()` coerce any corrupt-but-valid-JSON content to defaults so a broken
state file can never brick startup.

```
{"last_project": path|null,
 "recents": [paths, cap 10, newest first, deduped],
 "projects": {path: {"open_tabs": [relpaths], "active_tab": int}}}
```

## Run detection (runctl.py)

`package.json` present → `scripts.dev` → `scripts.start` → else `*.csproj` in
root → else first `*/*.csproj` alphabetically (`--project <dir>`) → else nothing
(Run disabled with an explanatory tooltip). Corrupt package.json → no run target.

## Testing model

- `tests/test_selfcheck.py` — displayless plain-assert suite (no framework). Pure
  paths only: state, runctl, gitstatus, filetree scan logic, VTE env/kill, and the
  run-window/respawn state machines via fake-self binding (real class methods bound
  to dict fakes; `panels.spawn` monkeypatched to simulate the pid landing).
- Widget/VTE behavior → throwaway `/tmp` probe scripts against the live display:
  `Gtk.init()`, real widgets, handlers driven directly where unrealized views don't
  emit signals, mainloop pumped with `GLib.MainContext`. Screenshot tooling on this
  box can serve stale frames — probes assert state numerically.