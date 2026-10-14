# CLAUDE.md — alpaca-code-rs

GTK4 desktop app wrapping the raw `claude` CLI: tabbed GtkSourceView editor +
file browser + three VTE panes (Agent = raw claude TUI, Terminal = `$SHELL`,
Output = npm/dotnet run), Run/Stop buttons, File menu. Stack: relm4 0.11 +
gtk4 0.11 (v4_22), sourceview5 0.11, vte4 0.10, glib/gio 0.22, gdk-pixbuf 0.22.

## Run / test

```
cargo test --manifest-path alpaca-code-rs/Cargo.toml    # displayless — THE test command
cargo build --manifest-path alpaca-code-rs/Cargo.toml   # cargo test does NOT refresh the runnable bin
alpaca-code-rs/target/debug/alpaca-code-rs [project-dir]  # no arg → last_project from state
```

System deps (Arch): `sudo pacman -S --needed gtksourceview5 vte4`.

## Status

- **Header menu row swap (2026-10-14, workbenches pending):** the
  File/Edit/Window/Help label buttons are GONE — one icon-mode chevron
  MenuButton (`chevron-down.svg` as custom child) at the header's left opens
  the workbench menu (New/Open | Save | Exit; `gio::Menu` sections = the
  separator lines; new/open/save = no-op stub actions, Exit = `app.quit`).
  Static popover attached ONCE at init — `refresh_menus`, `HeaderButtons`,
  `no_arrow` and the old menu-model fns are deleted (the open/recent/
  minimize/close/about/edit-op ACTIONS stay registered, surface-less, for
  the workbench phase). Menu test: `workbench_menu_sections_labels_actions`.

- **S1/S2/S3 complete (2026-10-09). S4 (multi-workspace tabs) complete
  (2026-10-14):** every open project = one `Workspace` (app.rs) — its own
  Editor/Panes/GitPanel controllers + hpaned assembly; the window child is a
  `GtkStack` (transition None = instant switch); hidden pages stay ALIVE
  (VTE children, git probes, diff pages keep running per project; NOTHING is
  ever re-rooted, so the old dirty-gate discard dialog is deleted). Header
  center = project pill strip + `+` (`MAX_WORKSPACES=3`; 4th open → dialog;
  close-pill shipped same day: the ✕ rides INSIDE the pill (image #17: one
  band covers label + ✕) — pill child box `[label(4px gap)✕]`, pill padding
  `1px 4px 1px 8px` (tab law: ✕ tucked right, label 8 left; image #18's
  right-inset tightening was REVERTED same day — the user wanted the right
  gap back), `.alpaca-projclose`
  = plain BOX + own GestureClick (C-probe: a nested Button NEVER fires — the
  outer pill's GestureClick claims the press, measured via CDP real-click on
  broadway); the ✕'s gesture CLAIMS ON PRESS (`current_sequence()` → Claimed)
  so the pill click is denied — CDP-click-rig proof: close + active swing +
  state prune all landed from one real pointer click. Label rule is
  DESCENDANT `.alpaca-projbtn label` now, x-dim.svg 14px; the ✕'s hover
  patch = SQUARE 18×18 (`min-height: 18px` + box `valign(Center)` in the
  21px band — a band-filling patch was a tall bar; image #18 ruling:
  measured patch insets 4/4/4/4 on the CDP hover rig), color #2b3448. Close =
  `AppMsg::CloseWorkspace(slot)` → `ws: Vec<Option<Workspace>>` TOMBSTONE:
  slots are never shifted or reused, so the closure-captured ws tags (the
  Add-time index every component output carries) stay valid forever — that
  IS the re-tag law's solution; `close_target` swings the active to the next
  live slot right else nearest left else zero-workspace (unit test
  `close_target_swings_right_then_left`). The kill is SYNCHRONOUS: App
  reads `panes.model().live_pids()` and `kill_tree`s each BEFORE dropping —
  a queued PanesMsg kill races relm4's shutdown branch in `futures::select!`
  and can be dropped with it (measured). Per-project `open_tabs` SURVIVE a
  close (reopen restores pages); closing all clears open_projects +
  last_project so boot's fallback can't resurrect the closed project. S4
  probe steps 5–6 assert the whole close (tombstone, swing, state prune,
  the re-tag proof: an OpenFile output still routes into slot 1 after
  slot 0 died).
- **relm4 close law (2026-10-14, measured — the S4 close-probe aborted on
  it): `ComponentSender::input()` PANICS once the runtime is shut down
  ("The runtime of the component was shutdown…", relm4 `channel/component.rs`
  expect) — and two paths fire after a shutdown on every close: (a) glib
  timers/`spawn_async` callbacks/`idle_add` closures that captured a
  component sender outlive their own `Controller` drop, (b) relm4's runtime
  task holds `rt_root`, so the widget tree finalizes INSIDE that task when
  its shutdown branch returns — `GtkNotebook` dispose reselects a page and
  emits `switch-page` from inside `drop_glue` (measured backtrace:
  trampoline mid-`drop_glue::<Box>`). So EVERY closure that can fire without
  user input (timers, child-exited, spawn-landed, sub-component output
  bridges, notebook switch-page, popover closed) sends via the guarded form
  `snd.input_sender().send(msg).is_err() → retire/no-op` instead of
  `.input()`; `Sender::send`/`.emit` return Options/warnings (never panic)
  but only `input_sender().send` is checkable from a `ComponentSender`.
  `#[relm4::component]` does NOT forward a `shutdown` hook (no "shutdown"
  in relm4-macros 0.11) — cleanup that must be deterministic can't ride it. The persisted
  `open_projects` is REBUILT from the live strip on every workspace ADD
  (only growth path — a deleted project's ghost can't evict a live tab;
  S4's state-open-set-live-truth pins it). All
  routing is index-tagged (`AppMsg`'s usize first field); `Workspace` holds
  the per-ws run mirror, snapshot, pending_commit. State: `open_projects`
  (cap 3, add order) + last_project doubles as the active project; boot
  reopens the strip then `BootActivate` swings to the remembered one.
  Wayland frame-callback freeze (measured, this stage): after a KWin restart,
  surfaces get no frame callbacks until touched, so GTK allocations FREEZE at
  the startup paint (~0.9-2.2s) — positional/live geometry is unreachable
  there; live probes need the running display before the freeze, or never restart KWin.

- App-id `io.alpaca.rs`. Reads/writes `~/.config/alpaca-code/state.json`
  (schema at the bottom of this file, atomic rename write).
- relm4 idioms (see src comments): `#[relm4::component(pub)]` (pub attr required,
  else E0446), init epilogue `let widgets = view_output!();` then `ComponentParts`
  (no `root` field), `T::builder().launch(x).connect_receiver(cb)` — connect_receiver
  RETURNS the Controller (no detach); outputs carry state (App can't query panes
  synchronously); `ComponentSender<C>` is generic over the COMPONENT.
- Widget ports must be probed live before trust: the respawn-band bug (spawn_pane
  clearing the budget every spawn — pane resurrection forever) shipped past 21
  green unit tests because the bookkeeping is widget-layer. Kill the pane child externally and count resurrections via
  `pgrep -P <app>`; that is the pane-exit path's only live gate.
- Rust probe envs (PROBE-ONLY chains in app.rs, never run with env unset):
  `ALPACA_PROBE_S2=<dir>` (workspace chain), `ALPACA_PROBE_S3=<dir>` (hpane
  width-step chain + tab strip), `ALPACA_PROBE_S4=<dir>` (multi-workspace
  chain: 2 opens + switch-survival + cap asserts + a close step (5–6:
  tombstone/swing/state-prune/re-tag proof); wsB/wsC are the passed
  dir's name-replaced siblings — each rig dir needs its letter file
  (a.txt/b.txt, plus wsB/b2.txt = the re-tag proof's OpenFile target);
  rig/`XDG_CONFIG_HOME` isolation per note
  below). S3's wrapper arms a 60ms root `queue_draw`
  damage keeper because an occluded/late-mapped Wayland surface gets no frame
  callbacks and its allocations freeze after the first paint (see S3 row
  above); it prints `PROBE step N`/tick traces, and the probe script kills the
  process at `PROBE S3 DONE` — the timer is never disarmed. Step timers each
  BREAK after firing (no idle re-arm hazard).
- `glib::TimeoutSource::…`/`timeout_add` in ms; spawn flags
  `SEARCH_PATH | SEARCH_PATH_FROM_ENVP`; env-marker override list
  (`runctl::pane_environ`, 9 markers) — runctl is the single source for pane env
  (marker drift = transcript-less child sessions).

## Rust modules (alpaca-code-rs/src/)

- `gitstatus.rs` — pure git porcelain/diff parser + subprocess callers
  (`branch_of/branches/switch/create_switch/changes/parse_*/diff_for/commit/ahead/
  commit_then_push/push/pull/group_tree`, git_run timeouts 5–120s).
- `treehover.rs` — HoverTree + press capture + toggle hit-test (press_xy, toggle_hit).
- `gitview.rs` — CHANGES view: tree + checkbox column + Select-all row + commit bar;
  CommitClicked validation guards + flight (Phase/Landed). apply() skips the
  store rebuild when the porcelain is identical and Filter() when the needle
  is unchanged — both are LOAD-BEARING: without them the 2s probe rebuilds a
  big repo's store every tick (31k rows ≈2.3s UI-thread each, misiuscode's
  unignored target/ measured it) and the mainloop saturates = hard hang.
- `branchmenu.rs` — branch status-bar pill + Switch/Pull/New-Branch popover
  (80/250ms hysteresis timers, generation-cancelled).
- `gitpanel.rs` — WORKSPACE/CHANGES mode card: shared search entry + status row
  (branch pill, dot, spinner, count) + 2s live probe; hosts file tree, changes
  view, branch pill.
- `editor.rs` (S2 additions) — diff pages: `OpenDiff` side-by-side tinted mono
  views (cross-linked scroll via allocation-notify re-link), `SaveOpen` flush
  chain, `Flushed` ack.
- `app.rs` (S2 additions) — Commit → `pending_commit` → SaveOpen → Flushed →
  StartCommit; `notify::is-active` → gitpanel RefreshGit.
- `vector.rs` (S1) — SVG → `VecIcon` paintable: gsk::Path::parse of the d
  attributes; art-only XML walker (`walk`: svg/g/path/rect/circle, no
  `<polygon>` — a golden count test fails visibly if art ever uses it);
  `icon(name)` caches the PARSED shapes (raw walk), not VecIcon objects —
  glib wrappers + gsk::Path are !Send so a static Mutex can't hold them;
  each call rebuilds a cheap VecIcon over the shared parse. Widget icons ride
  the paintable; the old pixel-path `badges::widget_icon` was deleted by the
  T2 swap (tree cells + letter chips stay pixbuf by the frozen-art ruling).
- `seti.rs` + `assets/icons/seti-*.svg` (2026-10-09 seti swap) — file-type badges from
  jesseweed/seti-ui: per-rule monochrome glyphs baked one-shot from
  mapping.less (rule color flattened in, `<style>`/class fills stripped — CSS
  fill beats baked attrs; gradients flatten to the rule color — seti renders
  these as one-color font glyphs). `icon_for(name)` replays mapping.less's
  cascade (LAST matching rule wins; ext rules ci, name rules authored-case,
  partial = substring); no match = `seti-default-white.svg`. Consumed ONLY
  through badges' pixbuf path (tree cells, tab badges, CHANGES rows) — never
  vector.rs's art-walker. Assets are frozen artifacts — the one-shot converter
  was never tracked (run outside the repo); to regenerate, re-author it from
  mapping.less rules rather than trusting any in-repo script.
- Timers that capture a component's Sender: `glib::timeout_add_local` — plain
  `timeout_add` requires Send and TreePath-bearing messages (filetree) are not.

## Invariants and gotchas (each is a measured ruling — trust them)

- `GtkSource.Buffer(text=…)` is born `modified=True` — an open must call `set_modified(False)`.
- `Gtk.Notebook.append_page` does NOT switch pages on this build — `set_current_page()` after.
- Theme styling of app widgets needs MORE than `border`: Breeze paints
 `notebook > header.top` as an inset box-shadow (`#4c4e51`) its own `border:none` doesn't
 cover — the strip grew a second light line directly above the app's dark border-bottom.
 Set `box-shadow: none` on `:top` AND `:backdrop` variants (measured rgb(76,78,81) leak).
 Breeze ALSO paints the notebook **node** (`notebook.frame` rule) with
 `box-shadow: inset 0 0 0 1px #4c4e51` — visible only where the page is transparent
 (the breadcrumb row band: the opaque source view covers it below), i.e. a 1px vertical
 gray at both card inner edges in the crumb row. Kill it too: `notebook { box-shadow: none }`
 (plus `.frame`/`:backdrop` selectors; survives the cairo switch → real paint op).
- Rule-carrying widgets: wanted insets go in CSS **padding**, never `margin` — margin sits
 outside the border box, so the breadcrumb label's `border-bottom` started 20px in from
 the card edge (measured rule from x27). `margin_start=20` deleted;
 `.alpaca-breadcrumb { padding: 2px 12px 2px 20px }` carries the inset.
- Probe discipline: an ablation means nothing unless the probe REPRODUCES first (the
 fullscreen page replica rendered clean; only a second application-id `io.alpaca.probe`
 real-app replica leaked). And re-derive window geometry from the KWin Scripting dump
 each capture — a missed/stale spectacle frame silently scans other windows' pixels and
 reads as a "clean" or "moved" artifact. Don't trust `spectacle -a` alone (stale
 committed frames on this WM) — assert probe state numerically, never eyeball pixels.
- Spawn flags must be `SEARCH_PATH | SEARCH_PATH_FROM_ENVP`, never `DEFAULT` (plain execv →
 bare `claude` dies ENOENT). Env: full environ with `~/.local/bin` appended to PATH — and
 VTE `spawn_async(envv=…)` MERGES envv onto the child's INHERITED environ, it never
 replaces it (measured: child kept the whole parent env). Omitting a key in `envv`
 scrubs NOTHING; to kill an inherited var (claude's ambient session markers turn the
 Agent pane's claude into a no-transcript child session — empty values satisfy its
 truthiness checks) OVERRIDE it to `""` in envv. Env build is pure `runctl::pane_environ`.
- VTE children are session leaders — kill trees with `killpg(getpgid(pid), SIGHUP)` +
 2s SIGKILL escalation (`panes::kill_tree`). `/proc/<pid>` existing is not proof of
 life (zombies); judge by exit status.
- Run state machine (panes.rs): the starting window covers the async gap before the pid
 lands; `has_running_run()` is true through it; Stop in that window sets `stop_pending`
 and the landed pid gets killed. Child-exit handling ignores our own cleanup exits so
 status stays truthful.
- Pane children auto-respawn on user exit, capped (3 per budget; a ≥60s-lived child re-arms;
  re-entering the tab re-arms; <3s deaths are broken children, never respawns — no loops).
- state.json contract (state.rs): load NEVER raises on any corrupt file — coerce
  missing/wrong types to defaults. A bad `state.json` must not brick startup.
- Editor refuses non-UTF-8 / NUL-containing files at open (`readable_text`) — a lossy
  buffer would be rewritten on Ctrl+S. Valid non-ASCII UTF-8 opens fine.
- Filetree rebuilds (`set_root`, search clear) reset the remembered `open` expansions —
  stale after any store clear; monitor refill prunes stale child expansions.
- Filetree (this build, measured): mutating the TreeStore under an EXPANDED row visually
  collapses the view (even deferred/idle fills); surgical INSERTs under expanded rows are
  safe. `view.row_expanded()` readback is deprecated + stale after mutations — expand state
  lives in filetree's own `open` set, updated by signal transitions. Children load before
  `expand_row` fires (never inside `row-expanded`). Tree store is 8 cols: 0 name, 1 abs path,
  2 is_dir, 3 fallback icon, 4 chevron, 5 badge pixbuf, 6/7 badge/icon visibility;
  `show-expanders=False`.
- Cell packing: with no `expand=True` cell, GtkCellRendererPixbuf (default xalign 0.5)
  floats to the column center — pack the text cell LAST with `expand=True` to pin
  chevron/badge/icon left.
- TreeView single click: `view.set_activate_on_single_click(True)`; dir rows toggle via
  `_open` membership (expand/collapse), file rows call `on_open`.
- GtkPaned propagates NO child minimums (window min = headerbar floor ~517px): any inner widget
  that refuses to shrink still gets clipped by an undersized window instead of raising the min.
  Every card stays compressible: `Notebook(scrollable=True)` both tabs stacks; ellipsize MIDDLE on
  editor tab names, breadcrumbs, tree text cells, browser status labels; header title ellipsize END.
  Cell props (ellipsize included) are cached at insert — set before any store fill.
- GTK 4.22 scrollable-notebook tab allocation (all measured): tabs sit at their MINIMUM width
  even with space to spare, and once the strip overflows, scroll mode's visible tail gets
  WIDER as the window narrows — text visibility is inversely coupled to window size.
  Also: a scrollable notebook's natural == its minimum, so GtkBox gives it only that
  minimum unless the nb has `hexpand=True` (900px wrap → 280px nb, measured).
  Fixes in editor.rs: `hexpand` on the nb + `TabDistributor` (BoxLayout subclass wrapping
  the nb) sets each tab-name label's min from the strip's live allocation inside its
  do_allocate — every tab = equal share `avail/n − CHROME(62)` capped at natural, recomputed
  on (width, n) change only. No fixed floor: one below the share re-enters scroll mode.
  Pre-map name floor stays `set_size_request(96, -1)` (GTK4 GtkLabel has no min-width-chars).
  Pane-tab labels have no ellipsize → min = full text, never starve; the S3 probe
  chain (ALPACA_PROBE_S3) asserts tab-strip geometry; default size clamps to the monitor.
- Editor tab badges: every tab carries its own file-type icon (user overrode the mockup's
  hash-for-inactive rule — a selected tab must not blank the others'); restamp via
  `_refresh_tab_state(index)` using the switch-page signal's index (get_current_page is
  stale inside that signal, measured).
- Icons are vector (`vector::icon(name)` → `VecIcon`, a `Gdk.Paintable` emitting
  Gsk fill/stroke nodes in viewBox units; GSK tessellates per frame at device scale — sharp
  at any display scale). Two resamples used to blur every pixbuf (measured 2026-10-04):
  librsvg scaling viewBox≠request geometry (40–45% partial-alpha pixels), then GtkImage
  drawing ANY pixbuf bilinearly upscaled to its 16px measure floor. Art rules: author each
  SVG 1:1 to its display grid (16 units for widget icons, 12 for the tree cells, paperclip
  keeps its lucide 24), strokes w2 on integer centerlines; VecIcon intrinsic = round(viewBox)
  soGtkImage allocates it 1:1 — a non-16 intrinsic (paperclip 24) needs `set_pixel_size(16)`.
  `Gdk.PaintableFlags` here: `.SIZE`|`.CONTENTS` (plural — `CONTENT` doesn't exist).
  Tree cells stay pixbuf: CellRendererPixbuf has no `paintable` property (cairo chips too) —
  exact-display-size art keeps them 1:1 at scale 1.
- Tab spacing (2026-10-05, twice revised): NO inter-tab margin — tab pills touch
  (user ruling; the old 2px side margins read as a dead 4px pill gap, edge-flush
  cancellation rules deleted with them). `margin: 0` must STAY in the tab rule — it is
  load-bearing: with the property absent, Breeze's `tab:checked` margins (`-3px` both
  sides) leak through and the pill box reshapes on every click (nudge) and pulls into
  the neighbours (probe: cells [134,134,131] → [131,134,134] across page switches;
  state-invariant only with margin 0 present). `TabDistributor`'s `TAB_CHROME` = 62 (no margin
  px) — without it the req overshoots shares and GTK falls back to unequal min-based
  allocation (measured).
- Breeze paints popovers via the `popover > contents` child node (bg + 1px
  #4c4e51 border + tail + 4px padding) — styling the `popover` node itself
  paints a DOUBLE frame (measured 17,23,35 ring around 28,31,34). Leave
  popovers bare; per-item classes (`.alpaca-branchitem`) are the styling
  surface. Label buttons must size their child labels' natural width (never
  max_width_chars/ellipsize inside a popover list — natural-width collapse
  shrink the popover to "…").
- Menubar hover pill: paint = header interior (32 today; the bar was 34 when this was
  first measured) − the button's own v-margins; Breeze's 16px child v-margin (= 18px
  paint) inflates if padding/min-height grow (bar 34→36/38, measured). Grow the
  paint by SHRINKING the child v-margin: paint tracks the button's internal box
  exactly (`interior − 2·margin`). Same phantom law as the run pill (buttons track
  child req +16).
- Header band heights EQUAL (2026-10-14, image #13, pixel-scan of the user's
  screenshot): selected project pill band ~24 (top-gap 4) is the reference;
  `.alpaca-header menubutton > button` margin `4px 0` → 24; `.alpaca-runpill` +
  `.alpaca-runpill button` min-height 24 → the run cells paint 24 too. All three
  bands share the header-interior center (valign/padding must stay 0 on pill text,
  image valign CENTER). Re-measure if the header interior ever changes. Hover
  bands are SQUARE = width EQUALS height, not right angles (user ruling
  2026-10-14, images #14-16, first misunderstood as radius-0 then corrected):
  height clamped at 24, so width comes to 24 via the 14px icons' horizontal
  padding `0 5px` (menubutton, `.alpaca-projadd`, runpill buttons); corner
  radii are the originals (6px menubutton/projadd-family/runcell, 8px runpill
  row shell).
- Run-cell hover band (2026-10-05, replaces the bg-image inset): the band is the
  cell's own `background-color` at FULL allocation — `background-image` could only
  be square or arc-bitten (bg-image is clipped to the cell's radius path: full-width
  = bite, inset = square, measured both ways). A bg-color paint is clipped to the
  same radius, so the shape comes from the cell's own `border-radius` — radius 0 =
  square by the 2026-10-14 ruling (was round). Band height = the run row's own
  min-height (24 since 2026-10-14) exactly BECAUSE the row stands that tall:
  `run_row.set_valign(Center)` in app.rs (default FILL would make the row allocate
  the header interior 32 — band would grow to 32). Keep this alignment if the
  header changes.
- Button metrics live at USER priority, and only after a zero-first sweep
  (2026-10-05): rules at APPLICATION (600) LOSE `button min-height` to Breeze on
  this build (32px button mins); USER (800) moves buttons to px-true. Then the
  headerbar injects a ~16px vertical additive on GtkButton that no button-side
  kill removes (`min-height: 0`, padding longhands, border, min-width, child
  margin — all measured alive; probe_dis 44/30 rows); it DIES with the
  everything-0 sweep `button, button > box, button > box > image` (no internal box
  exists — Button→Image is the real tree — but the sweep's child-node zeroing is
  what removes it), after which the wanted shape re-raises in a later same-block
  rule: `min-height: 28px; padding: 0 4px` → measured 22x28 button, cells 22x28,
  header 32(+2 border) in the real Window replica. Icons px14 via
  `_img(ipy, 14)` (VecIcon repaints sharp); play.svg re-drawn so glyph-ink
  center = box center (was +1.5 right; mockup icons are centered).
- Card junctions (2026-10-05, revised same day): the V gutter between editor and
  console cards is BACK by user reversal — the override
  `paned.vertical > separator { min-height: 0 }` is deleted; BOTH splits take the
  shared `paned > separator` 6px transparent-gutter rule, and the drag knob is
  resize-grabbable again (positions stay set in app.rs, never persisted; the
  6px comes out of the panes' area — position 592 = start-child height, so the
  editor card keeps ~593 and the seam's y is unchanged). Probe note: FULLSCREENING the probe window
  collapses the headerbar to a 12×0 alloc (GTK/KWin interplay on this build) — verify
  header stuff on a WINDOWED replica; header child allocs read 0 on this build even
  when painted (locate by pixel pattern, the struct dump is unreliable there).
- CSS/paint changes need a REAL restart — a second spawn forwards
  to the running primary (GtkApplication single-instance: the spawn exits, the old
  process gains the window) so the user keeps looking at the OLD css and "nothing
  changed". Kill the old process first (exact pid, not pkill -f), then relaunch.
- Run/Stop icon centering: a constant-size GtkImage under the default align lands at
  its content-box TOP (ink rides 7px high in the 32px cell) — set `set_valign(CENTER)`
  on the image (app.rs `_img`): alloc centers to (h−16)/2 and the ink centers match
  the band center to the pixel (measured 16.5 vs 16.5; play+stop agree). Alignment only:
  image min stays 16 → bar stays 34. Gotcha: a widget alloc probe can PRINT stale
  pre-layout values after an align change (requests unchanged → GTK skips re-alloc);
  freshly-built windows print true, resized/repainted ones paint true — verify PAINT
  pixels, not prints.
- Tab-text vertical centering: GTK centers the label's LINE BOX, but glyph ink hangs
  ~1.3–1.8px below line-box center (font ascent overhang, measured in a pixel capture) —
  so centered labels read 1-2px low in tall pills. Fix = `padding: 0 0 3px 0` on the
  label (`.alpaca-tabname`/`.alpaca-panetabname`); symmetric padding does nothing (the
  centered box shifts with it) and GtkLabel ignores Pango rise attrs — measured: ink
  offsets +1.82/+1.33 → +0.32/−0.17.
- Tree-icon vertical centering (file browser): pixbuf cells center on the line box while
  glyph ink hangs ~1px below → every filetype icon (docs, chips, folders) read ~1px high
  vs filenames at the 22px row. Fix = bottom-pin via `yalign` on the three pixbuf
  renderers (filetree.rs, set BEFORE inserts — cell props cache at insert, so post-set
  no-ops: measured twice): chips/symbolic 1.0 (+1px, 2px slack), chevron 2/3 (+1px, stays
  flush with folders). Shift law measured at slack 10: shift = slack·(yalign−0.5) exact on
  both `pixbuf` and `icon-name` render paths. Post-fix modes −0.04/+0.12 (was −1.04/−0.88).
  Re-measure if row height ever changes (slack = row − 2·ypad − 16).
- THE centering law (recurring class, every complaint traces here): each render channel
  centers a different BOX than the eye reads — the eye anchors on the dense-ink band
  (x-height for lowercase, caps band for all-caps) plus the baseline (text baseline = row
  top+15 exact in 22px tree rows). Channels: Pango labels/tabs (ink hangs — CSS
  padding-bottom 3px, above), pixbuf cells (yalign, above), cairo chip letters (badges.rs
  `render`: ink-box centering pulls LOWERCASE labels ~1.5px high because the descender-
  tipped box drags the average — fix centers the bowl band instead, baseline canvas 11 =
  row_top+15; caps labels are already exact since caps = the band; measured −1.5→≤0.15).
- CHANGES-panel gaps (2026-10-05, gitview.rs): WORKSPACE sets xpad 2 on every tree cell —
  4px art-to-art; CHANGES' chip/badge/name cells had none (0-2px gaps) and the blank
  chevron/chip slots in dir/Select-all rows floated the checkbox ~20-35px. Fix = xpad 2
  everywhere + chip/badge cells PER-ROW INVISIBLE on rows without chips (bind `visible`;
  this GTK drops a hidden cell's width per row — probe: hidden row's text sat 20px left
  of its twins). Measured after (column-run profile of replica shots): dir/file/Select-all
  gaps read 8-10px with tile edges eaten ~2px per side by antialias thresholds = the same
  4px WORKSPACE shows; badge→name parity 6 vs 6-9; chip bottom-pins −1px vs WORKSPACE.
  Amber-chip edges drop below sat thresholds before blue tiles — color, not layout.
- GtkSourceView style schemes: languages address styles through the `def:*` namespace
  (lang defs say `<style name="keyword" map-to="def:keyword"/>` and the render path calls
  `get_style("def:keyword")`) — defining only bare names (`keyword`) leaves ALL tokens in
  the base text color (measured: attached scheme + attached language still rendered plain).
  Keep `def:*` the primary definitions; bare names are aliases.
- Window buttons (minimize/maximize/close): the headerbar IS an internal `Gtk.WindowControls`
  (wrapped in a `WindowHandle` — `headerbar.get_first_child()` only shows that wrapper, so
  never pack your own WindowControls; it duplicates). KDE Wayland supplies no decoration
  layout — style.rs must force `gtk-decoration-layout` or buttons stay hidden.
- CSS: never give titlebuttons a `background` rule — Adwaita paints their icons via
  background-image, so `background: transparent` (e.g. a broad `.alpaca-header button`)
  blanks the window buttons entirely; scope button styling to `menubutton > button` etc.
- `GLib.timeout_add_seconds` never fired under `app.run()` on this box (twice measured);
  use `GLib.timeout_add(ms, cb)`.
- Label-mode `GtkMenuButton`s draw a real 16×24 `down` arrow widget that stays visible
  even when the glyph reads as invisible (menubar labels sat 38px apart: 16px arrow +
  20px padding). `set_always_show_arrow(False)` is a no-op on this build; CSS
  `display:none` selectors (`arrow`, `icon.down`, `button > box > icon`) don't match it
  either — fix = walk toggle → child box → css class `down`, `set_visible(False)`
  (stable across set_popover/set_label). Icon-mode buttons hide the arrow themselves.
  (2026-10-14: no label-mode MenuButton survives — the header's one menu button is a
  custom-child icon button; the walk law is kept for future label menus.)
- Uniform pill insets belong on the tab's head Box (`.alpaca-panetab { padding: 0 12px }`,
  never on the label: label-side padding leaves the leading icon flush with the pill
  edge (0px) while the text keeps ~9px — the "icon cramped, text floating" look.
- A raised box around a tab's ✕ in a screenshot is just `.alpaca-close:hover` — check
  the pointer position before reading it as stray styling.
- Gap audits: measure, don't squint. Dump alloc-vs-min per widget node (buttons: alloc
  minus child min = injected space), then pixel column-run profiles of a fresh
  screenshot to find glyph gaps. Mockup PNGs (n_*.png) are crops at differing zooms
  (~1.5–2.2× the app's 1×) — anchor each crop on a known value ("File" label ≈26px,
  header 46px) before converting its numbers.
- No programmatic toplevel resize in GTK4; default splits are `set_position(594)` / `(1181)`.
- `grab_focus()` on this build (4.22.5) SELECTS ALL when the entry has text and wasn't
  already its root's focus (probe: (0,11) on 'hello'); re-grabs of an already-focused
  widget short-circuit → no re-select. Two consequences (branch entry, measured):
  never hand `grab_focus` to `GLib.idle_add` (it returns True → re-arms FOREVER and
  re-grabs between keystrokes), and clear the entry before the one grab so no grab
  ever lands on text (stale reopen text would otherwise pre-select).
- Popovers are css-DOM CHILDREN of their `set_parent` widget: app USER-tier
  `X label` descendant selectors PIERCE them (pill's `.alpaca-open label`
  out-specced `.alpaca-hint` (0,2,1 vs 0,1,0) → hint rendered bright). Law:
  open/hover text color rides ON the button (label-inherit); every popover child
  label carries its own explicit rule. Breeze paints no popover label colors.
- Diff-page scrollbar hiding (measured 2026-10-13, two dead approaches first):
  `PolicyType::Never` is NOT "hide the bar, keep scrolling" — it UNCLAMPS the
  child, the sw's natural size passes the child's FULL content height through
  the page/notebook min chain and the window grows until upper==page_size on
  BOTH axes (nothing scrollable anywhere; measured viewport 1740px == content).
  And GTK4 css has NO `display` property on this build (`Theme parser error:
  No property named "display"` — a `display:none` scrollbar rule parses but is
  dead code, caught only when the probe logs were grepped for parser errors).
  The working hide = WIDGET visibility: walk the ScrolledWindow's internal
  `GtkScrollbar` children and `set_visible(false)` (menu-button-arrow walk
  pattern) — policies stay Automatic, viewport stays clamped, overlay reveal
  (css-class machinery) does not resurrect it; `editor.rs` `hide_scrollbars`.
- Diff-page scroll-pair asserts live at S2 chain step 9 (`ProbeDiffScroll`,
  `diff-*-` asserts at editor.rs `probe_diff_scroll_asserts`): step 4 (+400ms
  post-open) was racy on the live display (span/values unsettled between runs,
  same binary), and step 12's SetWorkspace closes every page — the pre-switch
  window (5-11) is the only settle slot. The diff sides render the WHOLE file
  (`diff_for` runs `-U999999999` — Rider-style full-file sides 2026-10-14:
  unchanged lines ride '=' rows with real numbers on both sides and keep FULL
  syntax highlighting — the same-day ctx-dim tag was taken back out; within a
  change band the first min(dels,adds) del/add PAIR on one shared row
  (`push_change_group`, user ruling: "lines match up as much as possible"),
  surplus pads the opposite side; a tail-append still fits the viewport — no
  span, asserts SKIP).
  Fixture law: 50-line base committed, worktree replaces lines 5-44 with long
  alternating-width lines — the BASE band is long too (both panes must
  h-overflow or sync-h/bar-drive can't run; ≈51 rows/side under the huge -U);
  wsB holds ZERO commits; wsA needs
  origin/main==HEAD (branch main). Each probe run commits wsA's change — full
  fixture reset before every run (the rigs live out-of-repo like the seti
  one-shot: build `/tmp/s2probe/` with a_base.txt = the 50-line base,
  a_new.txt = the long-line modified band, then: `cd /tmp/s2probe && rm -rf
  wsA/.git wsA_origin && git init -q --bare wsA_origin && git init -q wsA &&
  git -C wsA config user.email probe@local && git -C wsA config user.name
  probe && git -C wsA remote add origin /tmp/s2probe/wsA_origin && cp
  a_base.txt wsA/a.txt && git -C wsA add a.txt && git -C wsA commit -qm
  baseonly && (git -C wsA branch -m master main) && git -C wsA push -q -u
  origin main && cp a_new.txt wsA/a.txt` (a partial reset is a BOOBY TRAP: a
  worktree restore that lands on a HEAD already holding the new content reads
  CLEAN and the run gets no diff page; the probe's ProbeDiffScroll also
  self-renews: it appends a line to root/a.txt when it looks wsA — production
  is env-guarded), and grep probe logs for `parser error` — a dead CSS rule
  prints one at startup.
- Probes must NOT assume the user's display is free: the app is single-instance
  PER DBUS SESSION (round-2 measured: a plain spawn while the user's app is
  alive forwards to it — env lost, no probe lines). Headless probe runs =
  `gtk4-broadwayd :N` (fresh number per run) + `dbus-run-session -- env
  GDK_BACKEND=broadway BROADWAY_DISPLAY=:N ALPACA_PROBE_S2=…` (own session →
  own instance); delete `/run/user/1000/broadway*.socket` between runs;
  pkill -x gtk4-broadwayd (measured: the binary/proc NAME is gtk4-broadwayd —
  `pkill -x broadwayd` matches NOTHING and orphans daemons, 5 collected in one
  afternoon). Probe rigs must also isolate state.json: `XDG_CONFIG_HOME=<rig dir>`
  — probe sessions that open projects write open_projects/last_project, which
  without isolation would leak into the USER's next startup tab strip.
- GTK widget pairing laws (all measured 2026-10-13 via broadway probe captures
 + app probe runs): (a) `notify::allocation` NEVER fires on this GTK —
  allocation-notify retries were inert and are gone from diff_link;
  a View owns BOTH its adjustments from construction and the sw binds those
  same objects to its scrollbars (view-getter == sw-getter measured
  pointer-identical, identity stable across refresh_diff's buffer swaps).
  (b) A ScrolledWindow CLOBBERS its child's adjustments at set_child — any
  `set_vadjustment` on a view inside a sw must ride AFTER that child-set (the
  sw's adjustment-watcher then rebinds its bars to the new adj; a share taken
  before set_child is flattened; a share reading a main's adj taken before
  THAT main's sw was built binds a stranded pre-clobber placeholder). Number
  panes rely on (b): they share their main's vadjustment OBJECT (lockstep, no
  push races, one restore_frac per side); their h adj stays private (page
  sizes differ — one adjustment can't hold two).
- Paned divider gotchas (measured 2026-10-13): the UNSET position with
  text-view children put the start child at its 1px minimum in-app (a lone
  probe's even split does not generalize to wrapped/nested panes) —
  one-shot 50% rides the sw's hadjustment `changed` (page_size landing =
  the split area's first allocation) and set_position must fire at IDLE: a
  set mid-allocation-cascade is discarded (position property 482 with
  children still at the unset 1|958 compute — the S3 same-step law).
- Stack-page paned law (measured 2026-10-14, S4): a workspace tab opened
  HIDDEN (non-current stack page) holds its constructed paned position
  property (1161) across boot and first map — either unallocated (property
  intact, allocated_width 0) or clamped at first allocation to
  `min(1161, allocated_width − 6px separator)` (broadway vfb is narrow —
  observed 966 at hpane-w 972, the clamp doing its job, NOT a recompute).
  Asserts must accept both (`pos == 1161.min(hw-6)` when allocated, else
  `pos == 1161`); a pos unrelated to either under a wide hpane WOULD be the
  real hidden-page recompute (re-apply set_position at switch).
- Diff line numbers (Task 4): numbers are the REAL @@-hunk file numbers
  (Hunk.old_start/new_start now parsed, `Sides.old_no/new_no`, 0 = pad/hunk
  gap); text = `diff_num_text` (right-aligned via mono spaces, field =
  `diff_num_field_width` = widest number); the number columns are unfocusable
  views sharing the main's vadj. Rust `\<newline>` string continuations strip
  the next line's leading whitespace — heredoc-style multiline diff fixtures
  lose context lines' leading space (`strip_prefix(' ')` then drops them as
  unknowns); fixtures go in as one line-escaped string.
- Number-pane width (2026-10-14, user shot: digits cut mid-glyph "23"→"2:"):
  the open-time `view.measure()` under-reported the mono field (h bar hidden →
  no recovery) — pane width = pango layout of `0`.repeat(field) via the view's
  css-resolved font (`diff_num_pane_width`), re-stamped in refresh_diff's
  renewal path (a refreshed side can gain a digit, 99→100; sw_num_l/r retained
  in DiffWidgets for it). Assert upgrade: `num-width` = allocated ≥ pango
  natural (`≥20` passed while clipped); `num-filestart` = first number row
  reads "1" (sentinel the huge `-U` stays).
- Diff-page polish (2026-10-14, user shot + broadway capture): untagged
  text-node areas render the theme's LIGHT default — the number panes carry
  no style scheme, so both panes painted WHITE; fix = css on the text node
  (`.alpaca-mono text { background-color: #0d1017 }` — tag tints and scheme
  styles draw OVER it; digits ride `.alpaca-diffnum` color with a text-node
  restate, since a theme rule directly on `text` beats node-inherited).
  The paned divider is `paned.alpaca-diffpaned > separator`: a 2px #2b3448
  bar centered in the shared 6px grabbable gutter. The standalone merged h
  bar (Task 3) is styled by INHERITING the reference v bar's css classes
  (copied at open_diff, "vertical"→"horizontal": the sw's overlay machinery
  puts `overlay-indicator` on them and Breeze styles by class; a bare
  Scrollbar paints Breeze's classic light trough). Probe capture rig:
  `ALPACA_PROBE_S2_HOLD=N` parks the chain after step N-1 with the diff page
  open (use 5); capture = chromium headless `--screenshot` against
  gtk4-broadwayd's http port (display+8080) — spectacle 6.7.5 now silently
  no-ops (rc=0, no file, no output) and KWin's ScreenShot2 is portal-gated
  (`NoAuthorized`) on this build; `ALPACA_PROBE_S2` must be a wsA-CONTAINING
  path (wsB = name-replace, not the parent /tmp/s2probe — wrong dir =
  diff-presence FAIL + zero commits).

## Persistence schema

`~/.config/alpaca-code/state.json` (atomic temp-file + rename write):

```json
{"last_project": "/path|null", "recents": ["/path", …cap 10, newest first],
 "open_projects": ["/path", …cap 3, opened-order], "projects": {"/path": {"open_tabs": ["rel/path", …], "active_tab": 0}}}
```