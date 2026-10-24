# Invariants and gotchas

Every claim below is a measured ruling on this box (GTK 4.22.x / relm4 0.11 / KDE Wayland). Change behavior at your own risk: re-measure before re-trusting.

## Process and environment

- The app is single-instance per DBus session: a second spawn of the binary exits and forwards to the running primary (which gains a stray window and re-bound actions, losing your env). Probes must run under `gtk4-broadwayd :N` + `dbus-run-session` (see [probes.md](probes.md)). Before relaunching for a fresh look, kill the old process by exact pid — `pkill -f` matches your own shell (exit 144).
- CSS/paint changes need a REAL restart — the user keeps looking at the old process's CSS otherwise.
- Spawn flags must be `SEARCH_PATH | SEARCH_PATH_FROM_ENVP`, never `DEFAULT` (plain execv → bare `claude` dies ENOENT). Pane env comes from `runctl::pane_environ` (single source; marker drift = transcript-less child sessions).
- VTE `spawn_async(envv=…)` MERGES envv onto the child's inherited environ, never replaces it: to kill an inherited var (claude's ambient session markers turn the Agent pane's claude into a no-transcript child session) OVERRIDE it to `""` in envv — omitting a key scrubs nothing.
- VTE children are session leaders: kill trees with `killpg(getpgid(pid), SIGHUP)` + 2s SIGKILL escalation (`panes::kill_tree`). `/proc/<pid>` existing is not proof of life (zombies) — judge by exit status.
- `glib::timeout_add_seconds` never fires under `app.run()` on this box (twice measured); use `timeout_add` (ms). Timers capturing a component Sender must be `timeout_add_local` — plain `timeout_add` requires Send, and TreePath-bearing messages are not.

## relm4

- `ComponentSender::input()` PANICS once the runtime is shut down ("The runtime of the component was shutdown…"). Two paths fire after shutdown on every close: (a) timers / `spawn_async` callbacks / `idle_add` closures that captured a sender outlive their Controller's drop, (b) the widget tree finalizes inside the runtime task's shutdown branch (`GtkNotebook` dispose reselects a page and emits `switch-page` from `drop_glue`). So every closure that can fire without user input sends via the guarded form `snd.input_sender().send(msg).is_err() → retire/no-op`, never `.input()`.
- `#[relm4::component]` has NO `shutdown` hook (missing from relm4-macros 0.11) — cleanup that must be deterministic can't ride it.
- Idioms: `#[relm4::component(pub)]` (pub attr required, else E0446); init epilogue `let widgets = view_output!();` then `ComponentParts` (no `root` field); `T::builder().launch(x).connect_receiver(cb)` — returns the Controller; outputs must carry state (App can't query components synchronously); `ComponentSender<C>` is generic over the COMPONENT.

## GTK core laws

- Rule-carrying widgets: wanted insets go in CSS **padding**, never `margin` — margin sits outside the border box (the breadcrumb label's border-bottom started 20px in from the card edge).
- Button metrics live at USER priority, and only after a zero-first sweep: rules at APPLICATION (600) LOSE `button min-height` to Breeze's 32px mins; the headerbar also injects ~16px vertical additive on GtkButton that dies only with the everything-0 sweep `button, button > box, button > box > image`, after which the wanted shape re-raises in a later same-block rule. Gotcha: a widget alloc probe can PRINT stale values after an align change (requests unchanged → GTK skips re-alloc) — verify PAINT pixels, not prints.
- Never give titlebuttons a `background` rule — Adwaita paints their icons via background-image, so a broad `background: transparent` blanks the window buttons; scope button styling to `menubutton > button` etc.
- Breeze paints the notebook header/node as inset box-shadows (`#4c4e51`) that `border:none` doesn't cover — kill with `box-shadow: none` on the header `:top`/`:backdrop`/`notebook.frame` variants (light line above the app's dark border + 1px vertical gray at card inner edges).
- Breeze paints popovers via the `popover > contents` child node — styling the `popover` node itself paints a DOUBLE frame. Leave popovers bare; per-item classes are the styling surface. Label buttons must size their child labels' natural width (never ellipsize/max-width inside a popover list — the popover collapses to "…").
- Popovers are css-DOM CHILDREN of their `set_parent` widget: user-tier `X label` descendant selectors PIERCE them (0,2,1 out-speccing 0,1,0). Law: open/hover text color rides ON the button (label-inherit); every popover child label carries its own explicit rule. Also: a popover cannot map on a HIDDEN ancestor — reveal the ancestor before opening (production hover-reveal makes this invisible in normal use).
- Label-mode `GtkMenuButton`s draw a real 16×24 `down` arrow that stays visible even when it reads invisible (`set_always_show_arrow(False)` is a no-op; css `display` doesn't exist on this build). Fix = walk toggle → child box → css class `down`, `set_visible(False)`. Icon-mode buttons hide the arrow themselves.
- CSS on this GTK has NO `display` property (`Theme parser error: No property named "display"` — dead code). Hide via widget `set_visible`. Always grep probe logs for `parser error` — a dead rule prints one at startup and otherwise hides.
- `GtkPaned` propagates NO child minimums (window min = headerbar floor): inner widgets that refuse to shrink get clipped instead of raising the min. The UNSET position puts the start child at its 1px minimum; setting position mid-allocation-cascade is discarded — set at IDLE, one-shot, off the first `changed` of a sw's hadjustment. `set_position` never persisted by app code (app.rs sets defaults).
- Stack-page paned law: a workspace shown on a HIDDEN GtkStack page holds its constructed paned position property across boot/first map — either unallocated (property intact, alloc 0) or clamped at first allocation to `min(saved, allocated−6px separator)`. Asserts must accept both.
- ScrolledWindow CLOBBERS its child's adjustments at `set_child`: any post-set `set_vadjustment` must ride AFTER the child-set. Number panes rely on this: they share their main's vadjustment OBJECT (lockstep, no push races); h stays private (page sizes differ). A share read before the owner's sw was built binds a stranded pre-clobber placeholder. Related: `set_buffer` relayout resets the adj value (identity survives) — re-apply the frac inside the changed burst.
- `notify::allocation` NEVER fires on this GTK — allocation-driven retries are inert. A View owns BOTH its adjustments from construction; the sw binds its bars to those same objects.
- `ScrolledWindow` with `PolicyType::Never` is NOT "hide the bar, keep scrolling" — it UNCLAMPS the child and passes full content height up the min chain (window grows until nothing is scrollable). Working scrollbar hide = walk the sw's internal `GtkScrollbar` children and `set_visible(false)`; policies stay Automatic.
- Scrollable GtkNotebook: tabs sit at MINIMUM width even with space to spare; once the strip overflows, scroll mode's visible tail gets WIDER as the window narrows (text visibility inversely coupled to window size); the nb's natural == minimum so GtkBox gives it only that unless `hexpand=True`. Fix in editor.rs: hexpand + `TabDistributor` (BoxLayout subclass wrapping the nb) sets each tab label's min from the strip's live allocation inside do_allocate — equal share `avail/n − TAB_CHROME(62)` capped at natural, recomputed on (width, n) change only; no fixed floor.
- Breeze's `tab:checked` margins (−3px) leak through unless the tab rule carries `margin: 0` — load-bearing, pill reshapes/nudges on every click without it.
- `Gtk.Notebook.append_page` does NOT switch pages on this build — `set_current_page()` after. `get_current_page` is stale inside the `switch-page` signal — use the signal's index argument.
- `GtkSource.Buffer(text=…)` is born `modified=True` — an open must `set_modified(False)`. Languages address styles through the `def:*` namespace: defining only bare names leaves ALL tokens in the base text color.
- Timer reset/cancel never calls `SourceId::remove` — it PANICS on an already-fired id (one touch+delete race kills the app). Cancellation is by generation: bump a counter and let the landed closure discard itself.
- Language tally (`filetree::dominant_language`, linguist-style): git repos tally `git ls-files -z --cached --others --exclude-standard` BYTE-weighted — gitignored files are NOT counted; generated artifacts are EXCLUDED by name + ext (`GENERATED_NAMES` e.g. package-lock.json, `GENERATED_EXTS` = ["lock"]) — they outweigh real sources; prose exts (md/rst/txt) skipped; ties resolve to the lexically smallest ext (BTreeMap); extensionless names and dotfiles never feed the tally (rsplit('.') turns ".gitignore" into a fake ext). No git repo → plain count-walk fallback (same exclude laws).
- Treehover press sentinel: px<0 = no press → never a toggle; y<0 = no hover (a legal-looking pointer otherwise paints a phantom row-0 band). Chevron slot geometry: x0 = cell_area.x + 16 (measured); the hit-test falls back to 24px natural width.
- Filetree: mutating the TreeStore under an EXPANDED row visually collapses the view (even deferred/idle fills); surgical INSERTs under expanded rows are safe. `view.row_expanded()` readback is deprecated + stale — expansion lives in filetree's own `open` set. Children load before `expand_row` fires (never inside `row-expanded`). Rebuilds (`set_root`, search clear) reset remembered expansions; monitor refill prunes stale child expansions. Tree store is 8 cols; `show-expanders=False`.
- Cell packing: with no `expand=True` cell, GtkCellRendererPixbuf (xalign 0.5) floats to the column center — pack the text cell LAST with `expand=True` to pin chevron/badge/icon left. Cell props (ellipsize, yalign) are cached at insert — set BEFORE any store fill.
- TreeView single click: `view.set_activate_on_single_click(True)`; dirs toggle via the open-set membership, files call on_open.
- `grab_focus()` on this build SELECTS ALL when the entry has text and wasn't already its root's focus; re-grabs of an already-focused widget short-circuit. Never hand `grab_focus` to `GLib.idle_add` (returns True → re-arms FOREVER, re-grabs between keystrokes); clear the entry before the one grab.

## Centering (the recurring class)

- THE law: each render channel centers a different BOX than the eye reads — the eye anchors on the dense-ink band (x-height lowercase, caps band for all-caps) plus the baseline (text baseline = row top+15 in 22px tree rows).
- Pango labels/tabs: centered on LINE BOX but glyph ink hangs ~1.3–1.8px below (ascent overhang) — fix = `padding-bottom: 3px` on the label; symmetric padding does nothing; Pango rise attrs are ignored by GtkLabel.
- Pixbuf cells center on the line box → ~1px high vs text: fix = `yalign` on the renderers (set before inserts). Shift law at slack 10: shift = slack·(yalign−0.5), exact on `pixbuf` and `icon-name` paths. Re-measure if row height changes (slack = row − 2·ypad − 16).
- Cairo chip letters (badges.rs `render`): ink-box centering pulls LOWERCASE labels ~1.5px high (descender-tipped box drags the average) — center the bowl band instead (baseline canvas 11 = row_top+15); caps labels are exact.
- GtkImage under default align lands at content-box TOP: `set_valign(CENTER)` on the image. Image min stays 16 → a 16px GtkImage paints on the 16px measure floor — bare paintables draw blurred/upscaled otherwise; a non-16 intrinsic needs `set_pixel_size(16)`.
- Band-height laws in the header: all hover bands SQUARE = width EQUALS height (height clamped by row min-height; width arrives via the icons' horizontal padding). Uniform pill insets belong on the pill's head Box, never on the label (label-side padding leaves the icon flush with the pill edge).
- Run-cell hover band = the cell's own `background-color` at FULL allocation (bg-image can only be square or arc-bitten — it clips to the cell's radius path). Row must `set_valign(Center)` in code or the band grows to the header interior height.

## Icons and assets

- All widget icons are vector (`vector::icon(name)` → VecIcon paintable, Gsk fill/stroke nodes in viewBox units, sharp at any scale): the old double-resample (librsvg viewBox mismatch + GtkImage bilinear upscale to the 16px floor) blurred every pixbuf. Art rules: author each SVG 1:1 to its display grid (16 units widget icons, 12 tree cells; paperclip stays lucide 24) — strokes w2 on integer centerlines (thin strokes read as mush — "TS" → "15", probe-measured); VecIcon intrinsic = round(viewBox). PaintableFlags: `.SIZE | .CONTENTS` (no `CONTENT`). Snapping a paintable needs a REAL upcast — `PaintableExt::snapshot` on the raw wrapper panics (glib `is::<T>()` assert).
- Tree cells / tab badges CHANGES rows stay pixbuf: CellRendererPixbuf has no `paintable` property; exact-display-size art keeps them 1:1 at scale 1.
- `vector::icon` caches the PARSED shapes, not VecIcon objects — glib wrappers + gsk::Path are !Send, a static Mutex can't hold them; each call rebuilds a cheap VecIcon over the shared parse.
- seti icons (`seti.rs` + `assets/icons/seti-*.svg`): per-rule monochrome glyphs baked one-shot from seti-ui mapping.less (rule color flattened in; class/CSS fills stripped; gradients flatten to the rule color). `icon_for(name)` replays the cascade — LAST matching rule wins; ext rules lowercase, name rules authored-case, partial = substring; no match = seti-default-white. Consumed ONLY through badges' pixbuf path, never the art-walker. Assets are frozen artifacts — the one-shot converter was never tracked; regenerate by re-authoring from mapping.less rules.
- `vector::walk` is art-only SVG (svg/g/path/rect/circle, no `<polygon>` — a golden count test fails if art ever uses it).

## State and persistence

- `state.json` load NEVER raises on any corrupt file — coerce missing/wrong types to defaults. A bad state.json must not brick startup. Writes are atomic (temp file + rename).
- Workbench files: the file STEM stays the uuid (`workbench_path`'s hex+dash sanitize law keeps arbitrary text out of paths); user names ride the separate `display` field (card label = `display.unwrap_or(stem)`, no migration for old files). Rename restores the file's mtime (card order = last save; a rename must not ride the top).
- Workbench delete = gio `File::trash()` first, unlink fallback on trash-unsupported mounts (never raises; recovery ceiling is the trash).
- Boot adopt law: `last_workbench` is adopted ONLY when its file actually loads — a stale one → no name → the next save mints fresh (no resurrection).
- Open tabs per project SURVIVE a workspace close (reopen restores pages). Closing all clears `open_projects` + `last_project` so boot cannot resurrect closed projects.
- `open_projects` is REBUILT from the live strip on every workspace ADD (only growth path — a deleted project's ghost can't evict a live tab).
- Editor refuses non-UTF-8 / NUL-containing files at open (`readable_text`) — a lossy buffer would be rewritten on Ctrl+S. Valid non-ASCII UTF-8 opens fine.

## Workspace closing / pids

- Close is SYNCHRONOUS: App reads `panes.model().live_pids()` and `kill_tree`s each BEFORE dropping. A queued PanesMsg kill races relm4's shutdown branch inside `futures::select!` and can be dropped with it (measured).
- Slots are never shifted or reused (`ws: Vec<Option<Workspace>>` tombstone) — that keeps every closure-captured slot tag valid forever.
- `connect_close_request` → send `SaveThenClose` guarded + return `Propagation::Stop` ALWAYS (Proceed lets GTK destroy before the queued msg runs); the handler saves then `root.destroy()` (destroy ≠ close: no re-emission).

## Git / commit

- Committing a selection that contains worktree-PRESENT but index-REMOVED paths (`git rm --cached`, file kept on disk) must never ride `git add`: git refuses index-removed + gitignored pathspecs (rc≠0, one bad name fails the WHOLE batch) and silently RE-TRACKS non-ignored ones; `git commit --only` can't carry them either (its temp index takes worktree content for pathspec paths, resurrecting kept files, and resets the real index to the new HEAD, erasing other staged work).
- Handling: `gitstatus::commit` classifies via `staged_removals()` (`git diff --cached --diff-filter=D -z` union the ¬present selection paths), never adds them, and commits via a temp index (`GIT_INDEX_FILE=<tmp>` copy + read-tree HEAD + `update-index --force-remove` removals + `--add` the rest, then plain `git commit`, then `git rm --cached --ignore-unmatch` to unify the real index — so unselected staged state survives). The common no-removals path stays add + `--only`. Regression test: `commit_keeps_untracked_kept_staged_removals`.
- Diff pages render the WHOLE file (`diff_for` runs `-U999999999`, the `DIFF_CTX` constant): unchanged lines ride '=' rows with real numbers on both sides, full syntax highlighting kept; within a change band the first min(dels,adds) del/add PAIR shares one row, surplus pads the opposite side. Line numbers are the real @@-hunk file numbers (0 = pad/hunk gap). Rust `\<newline>` string continuations strip the next line's leading whitespace — multiline diff fixtures lose context lines' leading space (`strip_prefix(' ')` then treats them as unknowns); fixtures go in as one line-escaped string.
- Parsing `-z` porcelain: split on NUL, index into bytes (names can contain anything); the interlock rule: a rename pair is one change-group entity.
- gitstatus subprocess reads use dedicated reader threads to avoid pipe-buffer deadlock (large diffs exceed the pipe buffer; a synchronous read deadlocks against the writer).

## UI behavior contracts

- The close ✕ in a project pill is a plain Box + own GestureClick CLAIMED ON PRESS — a nested Button never fires (the outer pill's gesture claims the press). The launcher card hover icons use the same shape; a popover whose ancestor unmaps gets unmapped (`popover_open_of` checks first).
- Pane children auto-respawn on user exit, capped (3 per budget; a ≥60s-lived child re-arms; re-entering the tab re-arms; <3s deaths are broken children, never respawns — no loops).
- Run state machine (`panes.rs`): a starting window covers the async gap before the pid lands; `has_running_run()` is true through it; Stop in that window sets `stop_pending` and the landed pid gets killed. Child-exit handling ignores our own cleanup exits so status stays truthful.
- gitview apply() skips the store rebuild when the porcelain is identical and Filter() when the needle is unchanged — without them the 2s live probe rebuilds a big repo's store every tick (~2.3s UI-thread each at 31k rows) and saturates the mainloop.
- Probe-only chains live in app.rs (fns + step tables) gated by `ALPACA_PROBE_*` env vars — never run with env unset; see [probes.md](probes.md).

## Deliberate ceilings (marked `ponytail:` in code history, kept here so nobody "fixes" them blind)

- `filetree` project scan / search are full directory walks with per-entry stat: ~100ms on ~100k files (the unignored `target/` case). Cache or ignore-walk only if saves or search start to feel slow.
- `gitstatus` RefreshDiffs re-runs the full-file diff every 2s probe tick per open diff page; the porcelain-identity dedupe skips identical REBUILDS only, never the diff cost. If a huge page ever lags, fingerprint the porcelain instead of re-diffing.
- Probe chains are env-gated blocks living inside UI code (app.rs/editor.rs) — extracted modules were considered and skipped: code-motion risk for zero behavior gain.