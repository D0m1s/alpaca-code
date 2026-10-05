# CLAUDE.md — alpaca-code

GTK4 desktop app (PyGObject, Python 3.14) wrapping the raw `claude` CLI: tabbed
GtkSourceView editor + file browser + three VTE panes (Agent = raw claude
TUI, Terminal = `$SHELL`, Output = npm/dotnet run), Run/Stop buttons, File menu.

## Run / test

```
bin/alpaca-code [project-dir]      # no arg → last_project from state, else empty workspace
python3 tests/test_selfcheck.py    # plain asserts, displayless — THE test command
python3 tests/responsive_probe.py  # no-clip min-widths; needs a display (prints skip when headless)
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
  - `badges.py` — icon/chip factory: `icon(name)` → vector paintables (widgets), pixbuf path (tree cells + cairo text chips)
  - `vector.py` — SVG → `Gsk` paintables for widget icons; one parse per file, no pixbuf in the loop
  - `data/alpaca-dark.xml` — editor style scheme; `data/icons/*.svg` — the art (source of truth)

## Invariants and gotchas (each is a measured ruling — trust them)

- Never import `gi.repository` before `gi_env.require(...)` sets the versions.
- PyGObject property-by-attribute assignment silently no-ops (`win.title = x` does nothing) —
  always use `set_title()`/property setters.
- PyGObject wrapper identity is unstable: `get_nth_page(i) is get_nth_page(j)` matches only
  when a closure happens to keep a wrapper alive — compare indices
  (`nb.page_num(page) == nb.get_current_page()`) instead of object identity. Measured
  via editor tab badges: the `is` check matched a closed-tab wrapper and gave the active
  tab the inactive-tab hash glyph.
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
 reads as a "clean" or "moved" artifact.
- This box's vte4 is Vte-3.91: `ge.require("Vte", ("4", "4.0", "3.91"))`.
 `spawn_async` accepts only the keyword form.
- Spawn flags must be `SEARCH_PATH | SEARCH_PATH_FROM_ENVP`, never `DEFAULT` (plain execv →
 bare `claude` dies ENOENT). Env: full environ with `~/.local/bin` appended to PATH — and
 VTE `spawn_async(envv=…)` MERGES envv onto the child's INHERITED environ, it never
 replaces it (measured: child kept the whole parent env). Omitting a key in `envv`
 scrubs NOTHING; to kill an inherited var (claude's ambient session markers turn the
 Agent pane's claude into a no-transcript child session — empty values satisfy its
 truthiness checks) OVERRIDE it to `""` in envv. Env build is pure `runctl.pane_environ()`
 (panels.get_env wraps it).
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
- Filetree (this build, measured): mutating the TreeStore under an EXPANDED row visually
  collapses the view (even deferred/idle fills); surgical INSERTs under expanded rows are
  safe. `view.row_expanded()` readback is deprecated + stale after mutations — expand state
  lives in filetree's own `_open` set, updated by signal transitions. Children load before
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
  Fixes in editor.py: `hexpand` on the nb + `_TabDistributor` (BoxLayout subclass wrapping
  the nb) sets each tab-name label's min from the strip's live allocation inside its
  do_allocate — every tab = equal share `avail/n − CHROME(91)` capped at natural, recomputed
  on (width, n) change only. No fixed floor: one below the share re-enters scroll mode.
  Pre-map name floor stays `set_size_request(96, -1)` (GTK4 GtkLabel has no min-width-chars).
  Pane-tab labels have no ellipsize → min = full text, never starve.
  `tests/responsive_probe.py` asserts the maxima + uniform shrinking allocs; default size clamps to the monitor.
- Editor tab badges: every tab carries its own file-type icon (user overrode the mockup's
  hash-for-inactive rule — a selected tab must not blank the others'); restamp via
  `_refresh_tab_state(index)` using the switch-page signal's index (get_current_page is
  stale inside that signal, measured).
- Icons are vector now (`badges.icon(name)` → `vector.VecIcon`, a `Gdk.Paintable` emitting
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
  state-invariant only with margin 0 present). `_TabDistributor.CHROME` = 62 (no margin
  px) — without it the req overshoots shares and GTK falls back to unequal min-based
  allocation (measured).
- Menubar hover pill: paint = header(34) − the button's own v-margins; Breeze's 16px
  child v-margin (= 18px paint) inflates if padding/min-height grow (bar 34→36/38,
  measured). Grow the paint by SHRINKING the child v-margin: paint tracks the button's
  internal box exactly (34−2·margin): `margin: 3px 0` → 28px pill (rows 3..30, pixel
  capture), matching the run-cell band; bar untouched. Same phantom law as the run pill
  (buttons track child req +16).
- Run-cell hover band (2026-10-05, replaces the bg-image inset): the band is the
  cell's own `background-color` at FULL allocation — `background-image` could only
  be square or arc-bitten (bg-image is clipped to the cell's radius path: full-width
  = bite, inset = square, measured both ways). A bg-color paint is clipped to the
  same radius, so corners stay ROUND by construction — no inset arithmetic. Band
  height = 28 exactly BECAUSE the run row stands only 28 tall: `run_row.set_valign(
  CENTER)` in window.py (default FILL would make the row allocate the header
  interior 32 — band would grow to 32). Keep this alignment if the header changes.
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
  resize-grabbable again (positions stay set in window.py, never persisted; the
  6px comes out of the panes' area — position 592 = start-child height, so the
  editor card keeps ~593 and the seam's y is unchanged). Probe note: FULLSCREENING the probe window
  collapses the headerbar to a 12×0 alloc (GTK/KWin interplay on this build) — verify
  header stuff on a WINDOWED replica; header child allocs read 0 on this build even
  when painted (locate by pixel pattern, the struct dump is unreliable there).
- CSS/paint changes need a REAL restart — a second `bin/alpaca-code` spawn forwards
  to the running primary (GtkApplication single-instance: the spawn exits, the old
  process gains the window) so the user keeps looking at the OLD css and "nothing
  changed". Kill the old process first (exact pid, not pkill -f), then relaunch.
- Run/Stop icon centering: a constant-size GtkImage under the default align lands at
  its content-box TOP (ink rides 7px high in the 32px cell) — set `set_valign(CENTER)`
  on the image (window.py `_img`): alloc centers to (h−16)/2 and the ink centers match
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
- GtkSourceView style schemes: languages address styles through the `def:*` namespace
  (lang defs say `<style name="keyword" map-to="def:keyword"/>` and the render path calls
  `get_style("def:keyword")`) — defining only bare names (`keyword`) leaves ALL tokens in
  the base text color (measured: attached scheme + attached language still rendered plain).
  Keep `def:*` the primary definitions; bare names are aliases.
- Window buttons (minimize/maximize/close): the headerbar IS an internal `Gtk.WindowControls`
  (wrapped in a `WindowHandle` — `headerbar.get_first_child()` only shows that wrapper, so
  never pack your own WindowControls; it duplicates). KDE Wayland supplies no decoration
  layout — `main._theme_setup` must force `gtk-decoration-layout` or buttons stay hidden.
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

## Verifying widget behavior

The suite is displayless and never instantiates VTE/widgets. Widget behavior is verified
with throwaway `/tmp` probe scripts: `Gtk.init()` on the live display (returns None — GTK4
void; never truthy-test it), construct the real widget, then drive handlers directly where
view machinery is unreliable (signals don't fire
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