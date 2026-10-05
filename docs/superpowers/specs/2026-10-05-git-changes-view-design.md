# Git Changes View — design spec

Date: 2026-10-05 · Status: brainstormed, user-approved in chat
Branches from: existing browser card (`filetree.py`), tabbed editor (`editor.py`), pure git module (`gitstatus.py`)

## Purpose

A WORKSPACE / CHANGES toggle in the File Browser card exposes the project's git state: list every dirty path (staged, unstaged, untracked — one merged list), check files, open a side-by-side syntax-highlighted diff, commit exactly the checked files, push the branch.

Success test: open the app, click CHANGES, see dirty files with status letters, click one to read its diff, check a subset, type a message, commit and push — no terminal.

## User decisions on record

- **List shape: tree with directories** (over flat list) — dir rows expand to changed files, dir checkbox cascades to children, file checkboxes are individual.
- **Actions: separate Commit and Push buttons** (over a single Commit & Push).
- Diff opens in the editor area like a normal file tab (`"Clicking on a file should allow you to see its diff (like opening a regular file)"`).
- WORKSPACE and CHANGES are text buttons: muted section-label styling, hover brightens, the selected one stays bright.

## 1. Tab row (filetree.py)

The static `WORKSPACE` section label becomes a horizontal row of two buttons (`WORKSPACE`, `CHANGES`) in the `.alpaca-section` typography. A zero-transition `Gtk.Stack` beneath the search bar swaps the tree view / changes view; selection state is a css class on the buttons (hover rule brightens, class rule holds it).

The search bar filters the active view: workspace mode = existing `scan_project` walk; changes mode = substring filter over the built change store (no re-scan).

## 2. Changes view (new gitview.py)

`Gtk.Box` in the card's Stack: `TreeView` (same `.alpaca-tree` look as the file tree) + commit bar. Store rebuilt (not surgically patched) per refresh — filetree's tree-view invariants (mutating under expanded rows collapses them) apply; a full rebuild is the boring correct choice. Follows filetree conventions: cell props set before inserts, text cell packed with `expand=True`, single-click activation.

Columns/row anatomy: `[chevron][type badge][name (expand)] [status letter] [toggle]` — name expands, so letter+toggle sit flush right. Dirs: folder badge, chevron, no letter, checkbox reflects descendants. Files: type badge, status letter colored (`M` amber `#f2c94c`, `A`/`U` green `#22c55e`, `D` red `#ef4444`, `R` blue `#2f80ed`).

First row: **Select all** with the same toggle rendered tri-state via the inconsistent attribute — checked when all file rows checked, inconsistent when some.

Checked state lives in `ChangesView._checked: set[str]` (file paths only):
- file toggle → add/remove path
- dir toggle → add/remove all descendant file paths
- select-all → add/remove every file path
- every refresh/reset (open view, workspace switch, list rebuild) resets `_checked` to all file paths (documented; selection does not survive rebuilds)

Row activation: file row click → open its diff. The toggle cell handles its own clicks (`activatable`) — that split is probe-verified on this build before relying on it (CLAUDE.md convention; activate_on_single_click is on).

Empty state: repo clean → the list shows a "No changes" row and the commit bar is disabled. Outside a git repo (or no git binary) the CHANGES button hides entirely — same policy as the branch widget in the statusbar.

Refresh triggers: view becomes active, workspace switch, window focus (extend the existing `notify::is-active` hook in window.py that already refreshes the branch), after commit, after push.

## 3. Diff pages (editor.py)

`Editor.open_diff(rel, old_lines, new_lines, del_idx, add_idx)` receives ready-built sides (gitview runs `diff_for` + `parse_unified` + `build_sides` on click; the editor carries no git knowledge) and builds a page that looks like a file tab (breadcrumb header + content), keyed by attribute `page.diff_of = abs path` instead of `page.path` — so:
- no clash with real file lookups: `_page_of` matches either `page.path` or `page.diff_of`
- persistence skips diff pages entirely (`get_open_state` includes only pages with a `path`)
- Ctrl+S / `save_active` / dirty-dot / badge restamp loops guard for pages without `buf`/`path` (diff pages carry neither; all page-iteration sites use `getattr`)

Page content: one `ScrolledWindow` holding a horizontal `Gtk.Box` of two `GtkSourceView`s (old left, new right) separated by a 1px divider. Both views scroll inside the one ScrolledWindow — no per-view adjustments, no sync code. `wrap NONE`, line numbers off (side-by-side padding would falsify them), views read-only.

Side construction from parsed hunks:
- hunk header `@@ -a,b +c,d @@` → dim gray row on both sides (keeps alignment)
- context line → both sides
- removed line → left, blank pad right
- added line → right, blank pad left
- both sides end up with EQUAL line counts by construction

Syntax highlighting: per-file `GtkSource.LanguageManager.guess_language` + the `alpaca-dark` scheme, same as `open_file`. Tints via per-buffer `Gtk.TextTag`s (`alpaca-diff-del` on left removed-line ranges, `alpaca-diff-add` on right added-line ranges; blank pads stay untinted); tint hexes live beside the diff builder in editor.py (`del ≈ #25181c`, `add ≈ #15261d`, tuned to sit below `#e6e8ee` text legibility on `#0d1017`).

Binary files / non-UTF-8 diffs: `_error()`-style notice instead of a page.

## 4. Git plumbing (gitstatus.py — stays pure, no gi)

- `changes(root) → list[(path, letter)] | None` — `git status --porcelain=v1 -z --untracked-files=normal`; `None` outside a repo / no git / timeout. Letter = staged char when set, else worktree char, else `U`.
- `build_sides(hunks, old_text, new_text) → (old_lines, new_lines, del_idx, add_idx)` — pure, turns parse+file content into the editor's two line lists + tint line indexes.
- `parse_unified(text) → hunk list` — own parser (stdlib only), tolerant of no-index and missing trailers.
- `diff_for(root, rel, is_untracked)` — tracked: `git diff HEAD --no-color -U3 -- <rel>` (index+worktree vs HEAD, the change the commit will carry). Untracked or no-HEAD fallback: `git diff --no-index --no-color -U3 -- /dev/null <abs>` (initial-commit case tested). Binary marker in output → signal to caller.
- `commit(root, paths, msg) → (ok, text)`: refuse empty message; `git add -- <paths>` then `git commit --only -- <paths> -m msg`. `--only` means exactly the named paths land in the commit; a file staged by the CLI *outside* the selection stays staged, untouched. Test proves it.
- `push(root) → (ok, text)`: env `GIT_TERMINAL_PROMPT=0` + subprocess timeout; plain `git push`, and when stderr says there's no upstream, `git push -u origin <branch>` (branch name from `git symbolic-ref`). Never raises; returns stderr text.

Threading (gitview.py): commit/push run in a daemon `threading.Thread`; results marshal back via `GLib.idle_add`; the bar's buttons disable while running.

## 5. Wiring (window.py)

`self.changes = ChangesView()` hosted in the browser Stack; `changes.on_open = lambda p: editor.open_diff(p)`; commit flow asks the editor to first `save` pages whose path is in the checked set (small `Editor.save_path(path)` helper); after commit/push: `changes.refresh()` + `tree.refresh_branch()`.

## 6. CSS (main.py)

Tokens follow the existing family: `.alpaca-tabbtn` (text-only, muted → hover bright), selected class stays bright; diff tint constants; commit bar entry styled like `.alpaca-search` at lower height; result line colors reuse the status dot palette (`#22c55e` ok / `#f2c94c` warn-red family `#ef4444` error).

## 7. Testing

`tests/test_selfcheck.py` (displayless, plain asserts):
- `parse_unified` on a fixture diff (multi-hunk, adds/removes/context) → exact hunk structure; `build_sides` → equal-length sides, correct tint line sets.
- Temp git repo via `tempfile`: `changes()` parse (untracked included), initial-commit (no HEAD) diff fallback, `commit()` `--only` semantics — stage a sibling file, commit one path, sibling stays staged and absent from the commit — empty-message refusal, `push()` offline failure returns error without raising, missing git binary → `None` not exception.

Widget behavior stays probe-based per CLAUDE.md (throwaway /tmp scripts, numeric asserts, no screenshots-as-truth): toggle-vs-row click routing, stack swap, cascade logic driven by calling handlers directly.

## Ceilings (accepted, noted for the future)

- Diff pages are point-in-time; no auto-refresh, no per-hunk staging.
- No real-file line numbers on diff sides (padding would lie); numbers return only with a custom gutter renderer.
- `git` subprocesses run as-is; auth relies on the ambient ssh-agent/credential setup (prompts suppressed by design).
- Huge changesets: no virtualization; the store rebuild is O(n) rows.