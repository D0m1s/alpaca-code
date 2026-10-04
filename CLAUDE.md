# CLAUDE.md — alpaca-code

GTK4 desktop app (PyGObject, Python 3.14) wrapping the raw `claude` CLI: tabbed
GtkSourceView editor + file browser + three VTE panes (Agent Console = raw claude
TUI, Output = npm/dotnet run, Terminal = `$SHELL`), Run/Stop buttons, File menu.

## Run / test

```
bin/alpaca-code [project-dir]      # no arg → last_project from state, else empty workspace
python3 tests/test_selfcheck.py    # plain asserts, displayless — THE test command
```

System deps (Arch): `sudo pacman -S --needed gtksourceview5 vte4 python-gobject`. No pip deps.

## Layout

- `bin/alpaca-code` — launcher (arg → `main.run`)
- `alpaca_code/` — package (import name is `alpaca_code`: hyphens can't be Python identifiers)
  - `main.py` — CSS, forced-dark theme, style-scheme path, Application, argv → root resolution
  - `window.py` — layout, `set_workspace` (dirty gate + `_set_workspace_now`), Run/Stop wiring, File menu, New Project
  - `editor.py` — GtkSource tabs, Ctrl+S, dirty dot, delete-watch dialogs, binary-file rejection
  - `panels.py` — VTE panes + run lifecycle: pid landing, Running/Exit statuses, stop, respawn budget
  - `filetree.py` — lazy tree, dir monitors, search, skip-list
  - `state.py` — `~/.config/alpaca-code/state.json` persistence
  - `runctl.py` `gitstatus.py` — pure (no gi imports); safe to test directly
  - `gi_env.py` — `require(ns, versions)`; must run before any `gi.repository` import
  - `data/alpaca-dark.xml` — editor style scheme

## Invariants and gotchas (each is a measured ruling — trust them)

- Never import `gi.repository` before `gi_env.require(...)` sets the versions.
- PyGObject property-by-attribute assignment silently no-ops (`win.title = x` does nothing) —
  always use `set_title()`/property setters.
- `GtkSource.Buffer(text=…)` is born `modified=True` — an open must call `set_modified(False)`.
- `Gtk.Notebook.append_page` does NOT switch pages on this build — `set_current_page()` after.
- This box's vte4 is Vte-3.91: `ge.require("Vte", ("4", "4.0", "3.91"))`.
 `spawn_async` accepts only the keyword form.
- Spawn flags must be `SEARCH_PATH | SEARCH_PATH_FROM_ENVP`, never `DEFAULT` (plain execv →
 bare `claude` dies ENOENT). Env: full environ with `~/.local/bin` appended to PATH.
- VTE children are session leaders — kill trees with `killpg(getpgid(pid), SIGHUP)` +
 2s SIGKILL escalation (`panels.Panes._kill_tree`). `/proc/<pid>` existing is not proof of
 life (zombies); judge by exit status.
- Run state machine (`panels.py`): `_run_starting` covers the async window before the pid
 lands; `has_running_run()` is true through it; Stop in that window sets `_run_stop_pending`
 and the landed pid gets killed. `_run_child_exited` ignores our own cleanup exits so
 status stays truthful.
- Pane children auto-respawn on user exit, capped (3 per budget; a ≥60s-lived child re-arms;
  re-entering the tab re-arms; <3s deaths are broken children, never respawns — no loops).
- state.py contract: `load()`/`project_tabs()` NEVER raise on any corrupt file — coerce
  missing/wrong types to defaults. A bad `state.json` must not brick startup.
- Editor refuses non-UTF-8 / NUL-containing files at open (`readable_text`) — a lossy
  buffer would be rewritten on Ctrl+S. Valid non-ASCII UTF-8 opens fine.
- Filetree rebuilds (`set_root`, search-clear) reset `_expanded` — remembered expansions
  are stale after any store clear; monitor refill prunes stale child expansions in `_fill_children`.
- No programmatic toplevel resize in GTK4; default splits are `set_position(594)` / `(1181)`.

## Verifying widget behavior

The suite is displayless and never instantiates VTE/widgets. Widget behavior is verified
with throwaway `/tmp` probe scripts: `Gtk.init()` on the live display, construct the real
widget, then drive handlers directly where view machinery is unreliable (signals don't fire
on unrealized widgets — call e.g. `fb._on_row_expanded(view, iter, path)` yourself).
Pump the mainloop: `GLib.MainContext.default().iteration(may_block=False)` in a time-bounded
loop. Don't trust `spectacle -a` screenshots alone (stale committed frames on this WM) —
assert probe state numerically.

## Persistence schema

`~/.config/alpaca-code/state.json` (atomic tmp + `os.replace` write):

```json
{"last_project": "/path|null", "recents": ["/path", …cap 10, newest first],
 "projects": {"/path": {"open_tabs": ["rel/path", …], "active_tab": 0}}}
```