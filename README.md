# alpaca-code

A minimal GTK4 desktop editor (Rust, relm4) that wraps the raw `claude` CLI. One dark window:
a tabbed code editor, a file browser, and a bottom console area running the real
`claude` TUI beside an Output pane and a plain terminal. Workbench launcher at boot.

## Features

- **Agent Console** — the raw `claude` TUI in your project root. Ctrl+C inside it
  cancels claude's current action, not the pane; exiting claude revives it (capped
  so a broken install can't respawn-loop).
- **Editor** — tabbed GtkSourceView with syntax highlighting (custom `alpaca-dark`
  scheme), dirty dots, Ctrl+S save, breadcrumb per tab, side-by-side diff pages,
  and a dialog when a file is deleted on disk. Binary/NUL/non-UTF-8 files are
  refused at open so a save can never mangle them.
- **File browser** — lazy tree (node_modules, .git, obj/bin/vendor skipped), live
  file monitors, filename search over the whole project, file count, git branch label.
- **Output** — `npm run dev`/`npm start` or `dotnet run` (detected from the project),
  double-click-Run protected, one Stop to kill the whole child tree.
- **Terminal** — your `$SHELL`, with `~/.local/bin` on PATH even when launched from a menu.
- **Projects** — up to three at once as workspace tabs; recents (10, deduped) and
  open tabs persist between launches.
- **Workbenches** — explicit sessions: Ctrl+Shift+S saves paned/scroll/tree
  layout to a workbench file; the boot **Launcher** lists every saved workbench
  as a card (project tiles with the saved language + git branch) — click opens
  the whole strip back up, the big `+` opens a folder as a fresh workbench.

## Install (Arch / CachyOS)

```
sudo pacman -S --needed gtksourceview5 vte4
```

## Run

```
cargo build                                  # refresh the bin
./target/debug/alpaca-code-rs                # the workbench launcher
./target/debug/alpaca-code-rs ~/some/project # open a specific project
```

## Shortcuts

| Key | Action |
|---|---|
| Ctrl+S | Save active tab |
| Ctrl+Shift+S | Save workbench |
| Ctrl+Shift+F | Focus the panel search |
| Ctrl+Q | Quit (tabs persist) |

## State

`~/.config/alpaca-code/state.json` — last project, recents, workspaces, window.
`~/.config/alpaca-code/workbenches/<uuid>.json` — one saved session layout each.

## Tests

```
cargo test
```

Agent-facing docs live in [AGENTS.md](AGENTS.md) and [docs/](docs/).