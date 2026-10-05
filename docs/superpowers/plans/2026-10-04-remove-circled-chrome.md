# Remove Circled UI Chrome — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Delete the four chrome clusters circled in the user's screenshot (editor-tab-bar right cluster, File-Browser "⋯", pane-tab-bar Ready/trash cluster, agent-console ask row) plus their now-dead code, CSS, and icon art.

**Architecture:** Pure subtraction across 5 small modules (`editor.py` 320 lines, `panels.py` 359, `filetree.py` 323, `window.py` 365, `main.py` CSS) + 5 unused SVGs in `alpaca_code/data/icons/`. No behavior is re-wired afterward: every deleted widget was mockup chrome whose only real function (File menu, run/stop state sync, git status) lives elsewhere and survives.

**Tech Stack:** GTK4 / PyGObject, GtkSourceView, VTE; Python 3.14, no pip deps. Test suite is displayless plain asserts: `python3 tests/test_selfcheck.py`.

**Spec:** the user's screenshot — red circles on (1) "Editor ⟩⧉" cluster top-right of the editor tab strip, (2) "⋯" in the File Browser title row, (3) "● Ready 🗑" right of the Agent Console / Output / Terminal tabs, (4) the bottom strip of the Agent Console: the "bypass permissions on… · PR #51 · ≡ 3 agents" line plus the "Ask the agent to make a change…" input row.

## Global Constraints

- `python3 tests/test_selfcheck.py` must print all PASS after every task (exit 0).
- Never import `gi.repository` before `gi_env.require(...)` (files already comply — plan adds no imports).
- No commits anywhere in this plan: the working tree already carries uncommitted v1.x work in these same files (editor/panels/window/main are `M` in git status); the user reviews and commits.
- Deletion, not refactor — where a deletion strands a helper, helper goes too (`ponytail: deletion over addition`).
- CLAUDE.md invariants untouched: attribute→property setter rule, PyGObject wrapper identity by index, `timeout_add` (never `timeout_add_seconds`), etc.

## Review Focus

Uncovered inputs most likely to bite a person using the app after this cleanup:

1. **Claude's own console footer ("bypass permissions on · PR #51 · agents")** is rendered by the claude CLI *inside* the VTE — app code cannot remove it, and no task tries. Expected behavior: it stays exactly as claude draws it; removing it is a claude-side setting outside this repo. Do not attempt to mask terminal rows app-side.
2. **Editor tab widths with no END action widget** — `_TabDistributor.do_allocate` reads `get_action_widget(END)` and already handles `None` (`act_w = 0`); expected: tabs simply gain the cluster's ~60px and distribute unchanged. Pinned by `tests/responsive_probe.py` (needs a display; prints skip when headless).
3. **Run/Stop button enable/disable** must keep working after the Ready chip is gone — it is driven by `window._on_run_status` via the `on_status` callback, not by the label widget. Pinned by existing `test_run_pid_landing_and_races` + `test_pane_respawn_capped` (they stub `_set_status`, unaffected by the widget deletion).
4. **`badges.icon()` on a deleted art name must still return `None`** rather than raise — existing `vector.icon("missing.svg") is None` test pins the mechanism; Task 4's grep guards no callers remain.
5. **File Browser git dot uses `.alpaca-status-dot ok/warn`** — those CSS rules must survive while the pane chip's rules die. Pinned by Task 4's grep + `test_main_css_palette_tokens` (all needles re-verified against post-deletion CSS).

---

### Task 1: Editor tab-bar right cluster ("Editor" hint + divider + panel-toggle)

**Files:**
- Modify: `alpaca_code/editor.py:68` and `:85-98`
- Modify: `alpaca_code/window.py:121-122` and `:194-202`

**Interfaces:**
- Consumes: nothing (this code *is* the dead hook).
- Produces: `Editor` no longer declares `on_toggle_panes`; `window.py` no longer assigns or calls `_toggle_panes`. `_TabDistributor` code (lines 37-38) stays AS-IS — `get_action_widget()` returning `None` is already handled.

- [ ] **Step 1: Delete editor.py block** — remove editor.py:85-98 exactly:

```python
        # mockup's right cluster: "Editor" hint + divider + panel-toggle
        side = Gtk.Box(spacing=6, margin_start=6, margin_end=4)  # + notebook header pad 0 → ~10 from edge
        side.set_valign(Gtk.Align.CENTER)
        self.nb.set_action_widget(side, Gtk.PackType.END)
        divider = Gtk.Box(css_classes=["alpaca-vdiv"]); divider.set_size_request(1, 16)
        side.append(Gtk.Label(label="Editor", css_classes=["alpaca-panel-hint"]))
        side.append(divider)
        self.pane_toggle = Gtk.Button()
        ppix = badges.icon("panel.svg")
        if ppix:
            self.pane_toggle.set_child(Gtk.Image.new_from_paintable(ppix))
        self.pane_toggle.set_css_classes(["alpaca-btn", "flat"])
        self.pane_toggle.connect("clicked", lambda b: self.on_toggle_panes())
        side.append(self.pane_toggle)
```

and editor.py:68 (`self.on_toggle_panes = lambda: None     # window hooks the run/stop pane flip here`).

- [ ] **Step 2: Delete window.py hook** — remove window.py:121-122:

```python
        self.editor.on_toggle_panes = self._toggle_panes   # mockup's panel-toggle in the editor tab bar
        self._panes_pos = None
```

and the whole `# --- pane toggle ...` method `window.py:194-202`:

```python
    # --- pane toggle (editor tab-bar panel button) -------------------------------
    def _toggle_panes(self) -> None:
        if self.panes.get_visible():
            self._panes_pos = self.vpane.get_position()
            self.panes.set_visible(False)
        else:
            self.panes.set_visible(True)
            if self._panes_pos:
                self.vpane.set_position(self._panes_pos)  # restored so the handle lands back in place
```

- **Function lost, accepted per spec:** hide/show of the bottom panes card is now unreachable — that button was its only trigger. User asked for removal; no replacement.

- [ ] **Step 3: Verify**

Run: `python3 tests/test_selfcheck.py` → all PASS, and:
`grep -rn "pane_toggle\|on_toggle_panes\|_panes_pos\|_toggle_panes" alpaca_code/` → no hits.

### Task 2: File Browser "⋯" menu button

**Files:**
- Modify: `alpaca_code/filetree.py:74-79` (and window.py's reference is cleaned in Task 3)

**Interfaces:**
- Consumes: nothing (the button's popover came from `window._refresh_menus`, which `getattr`s it — Task 3 removes that reference; until then `getattr(self.tree, "menu_btn", None)` returning `None` is already safe).

- [ ] **Step 1: Delete filetree.py:74-79** — the button block only; the title label keeps `hexpand=True`, margins unchanged:

```python
        self.menu_btn = Gtk.MenuButton()
        more = badges.icon("more.svg")
        if more:
            self.menu_btn.set_child(Gtk.Image.new_from_paintable(more))
        self.menu_btn.set_css_classes(["alpaca-btn"])
        head.append(self.menu_btn)
```

- **Function lost, accepted:** the browser's ⋯ showed the same File menu as the headerbar (Open Project / Open Recent / New Project / Quit — `window._refresh_menus` packs it in duplicates, zero unique items). The headerbar File menu remains.

- [ ] **Step 2: Verify**

Run: `python3 tests/test_selfcheck.py` → all PASS.
Run (expected to still hit only window.py pending Task 3): `grep -rn "menu_btn" alpaca_code/` → only `window.py:268`.

### Task 3: Pane tab-bar cluster (● Ready + trash + ⋯) and the ask row

**Files:**
- Modify: `alpaca_code/panels.py:71`, `:80-103`, `:104-134` (partially), `:137-162`, `:164-179`, `:193-203`, `:335-340`
- Modify: `alpaca_code/window.py:266-271`

**Interfaces:**
- Consumes: `on_status` callback (stays) — window's run/stop sync depends on it.
- Produces: `Panes` no longer has `more_btn`, `dot`, `status_label`, `trash`, `ask_entry`, `_build_ask_row`, `_on_prompt`, `clear_active`, `_clear_active`. `_set_status(text, cls)` keeps its exact signature (tests stub it by name) but becomes a pure `on_status` passthrough.

- [ ] **Step 1: Delete panels.py:80-103** (the whole strip built into the notebook's END action widget) and **panels.py:132** (`self.nb.set_action_widget(strip, Gtk.PackType.END)   # status lives in the tab bar, mock-style`):

```python
        strip = Gtk.Box(spacing=6, margin_start=12, margin_end=6, margin_top=0, margin_bottom=0)  # + header pad 12 → ~18
        strip.add_css_class("alpaca-status-row")
        self.dot = Gtk.Box(css_classes=["alpaca-status-dot"])
        self.dot.set_valign(Gtk.Align.CENTER)
        self.status_label = Gtk.Label(label="Ready")
        self.status_label.add_css_class("alpaca-status-label")
        self.trash = Gtk.Button()
        tpix = badges.icon("trash.svg")
        if tpix:
            self.trash.set_child(Gtk.Image.new_from_paintable(tpix))
        else:
            self.trash.set_child(Gtk.Image(icon_name="edit-clear-all-symbolic"))
        self.trash.add_css_class("alpaca-close")
        self.trash.set_tooltip_text("Clear pane")
        self.trash.connect("clicked", self._clear_active)
        self.more_btn = Gtk.MenuButton()          # window packs its popover in
        mpix = badges.icon("more.svg")
        if mpix:
            self.more_btn.set_child(Gtk.Image.new_from_paintable(mpix))
        self.more_btn.add_css_class("alpaca-btn")
        strip.set_valign(Gtk.Align.CENTER)
        strip.append(self.dot); strip.append(self.status_label)
        strip.append(self.trash)
        strip.append(self.more_btn)
```

- **Function lost, accepted:** visible run status ("Ready"/"Running"/"Exit N" chip — state is still computed and fed to `on_status`, just no longer displayed), trash = clear-pane scrollback, and a second File-menu duplicate behind ⋯. Claude's *own* status line in the VTE (circle 4a) is untouched by anything here — see Review Focus #1.

- [ ] **Step 2: Delete the ask row** — panels.py:134 (`self.append(self._build_ask_row())`), the whole `_build_ask_row`/`_on_prompt` pair (panels.py:136-179):

```python
    # ---- ask-the-agent row ------------------------------------------------------
    def _build_ask_row(self) -> Gtk.Box:
        """Design's input row inside the pane card: '›' | entry | paperclip | blue send."""
        row = Gtk.Box(spacing=6, margin_top=1, margin_bottom=2, margin_start=6, margin_end=6)
        row.set_css_classes(["alpaca-ask"])
        row.append(Gtk.Label(label="›", css_classes=["alpaca-askprompt"]))
        div = Gtk.Box(css_classes=["alpaca-vdiv"]); div.set_size_request(1, 16)
        div.set_valign(Gtk.Align.CENTER)
        row.append(div)
        self.ask_entry = Gtk.Entry(hexpand=True,
                                   placeholder_text="Ask the agent to make a change…",
                                   css_classes=["alpaca-askentry"])
        self.ask_entry.connect("activate", self._on_prompt)
        row.append(self.ask_entry)
        clip = badges.icon("paperclip.svg")     # lucide 24-grid art — pin to 16 (intrinsic would measure 24)
        if clip:
            cimg = Gtk.Image.new_from_paintable(clip)
            cimg.set_pixel_size(16)
            row.append(cimg)
        send = Gtk.Button(css_classes=["alpaca-send"])
        spix = badges.icon("send.svg")
        send.set_child(Gtk.Image.new_from_paintable(spix)
                       if spix else Gtk.Image(icon_name="document-send-symbolic"))
        send.set_tooltip_text("Send")
        send.connect("clicked", lambda b: self._on_prompt(self.ask_entry))
        row.append(send)
        return row

    def _on_prompt(self, *_a) -> None:
        text = self.ask_entry.get_text().strip()
        if not text or not self.root:
            return
        self.ask_entry.set_text("")
        self.nb.set_current_page(0)               # bring the Agent Console forward
        if "agent" not in self._spawned:          # quiet console → spawn it first
            self._spawned.add("agent")
            self._respawns.pop("agent", None)     # deliberate re-entry re-arms the exit budget
            self._spawn_pane("agent")
        def feed():
            # tty input buffers kernel-side, so even a pre-TUI feed queues fine;
            # the delay only lets a just-spawned claude mount before Enter lands
            self.agent.feed_child((text + "\n").encode())
            return GLib.SOURCE_REMOVE
        GLib.timeout_add(400, feed)
```

- Keep everything `else` in the `__init__` notebook block (the `while` loop over `(self.agent, self.out, self.term)`, VTE wiring, `add_page` calls, `switch-page` connect) exactly as is. Interaction stays: claude runs in the VTE and accepts keyboard input directly, as it does today when focus is in the terminal.

- [ ] **Step 3: Reduce `_set_status` (panels.py:193-203)** — widget chrome gone, keep the pure passthrough (tests stub this method by name):

```python
    def _set_status(self, text: str, cls: str = "") -> None:
        self.on_status(text, cls)
```

- [ ] **Step 4: Delete `clear_active`/`_clear_active` (panels.py:335-340)** — trash handler, its only caller dies in Step 1:

```python
    def clear_active(self) -> None:   # trash button
        self._clear_active(None)

    def _clear_active(self, *_a) -> None:
        t = (self.agent, self.out, self.term)[max(self.nb.get_current_page(), 0)]
        t.reset(True, True)
```

- [ ] **Step 5: Simplify window.py `_refresh_menus` (window.py:266-271)** — both ⋯ popovers are gone (Task 2 + Step 1); the loop over three buttons collapses:

```python
    def _refresh_menus(self) -> None:
        """File menu in exactly one place: the headerbar File button."""
        self.menubtn.set_popover(Gtk.PopoverMenu.new_from_model(self._build_menu()))
```

- [ ] **Step 6: Verify**

Run: `python3 tests/test_selfcheck.py` → all PASS (run lifecycle tests stub `_set_status`; respawn-cap test calls it the same way).
Run: `grep -rn "more_btn\|status_label\|ask_entry\|_build_ask_row\|_on_prompt\|clear_active\|\"alpaca-status-row\"" alpaca_code/` → no hits, and `grep -rn "status_label" tests/` → no hits.

### Task 4: Dead CSS and unused icon art

**Files:**
- Modify: `alpaca_code/main.py` (`CSS` block, lines 81-88 + 132-146)
- Delete: `alpaca_code/data/icons/{more,panel,trash,paperclip,send}.svg`

**Interfaces:**
- Consumes: grep knowledge from Tasks 1-3 — no code references remain.
- Produces: `CSS` shrinks; `.alpaca-status-dot` (+ `ok`/`warn`) STAYS — File Browser's git dot uses it (filetree.py:139-194). `#2f80ed`/`min-height: 30px`/`border-radius: 4px` needle claims in `test_main_css_palette_tokens` unaffected.

- [ ] **Step 1: main.py CSS — delete these rules** (keep neighbors):

```css
.alpaca-status-label { color: #8a93a6; font-size: 12px; }
.alpaca-status-label.ok { color: #22c55e; }
.alpaca-status-label.err { color: #ef4444; }
.alpaca-status-dot.err { background: #ef4444; }
```

```css
.alpaca-panel-hint { font-size: 12px; color: #8a93a6; }
.alpaca-vdiv { min-width: 1px; background: #2b3448; }
```

and the entire ask-row block (main.py:135-146):

```css
/* --- ask-the-agent input row (~36px, hairline #2b3448) --- */
.alpaca-ask { background: #10151f; border: 1px solid #2b3448; border-radius: 8px;
              min-height: 30px; padding: 0 4px 0 10px; }
.alpaca-ask entry { background: transparent; border: none; box-shadow: none;
                    color: #8a93a6; font-size: 13px; }
.alpaca-ask entry:focus { color: #e6e8ee; }
.alpaca-askprompt { color: #8a93a6; font-size: 15px; }
/* icon button min = max(css-min, icon) + padding: 13px icon needs 7px side
   padding to land 30×30 (measured) */
.alpaca-send { background: #2f80ed; border: none; border-radius: 8px;
               min-width: 2px; min-height: 2px; padding: 7px; }
.alpaca-send:hover { background: #4a92ee; }
```

- [ ] **Step 2: delete icon art with no remaining callers** (Task greps prove it):
`rm alpaca_code/data/icons/more.svg alpaca_code/data/icons/panel.svg alpaca_code/data/icons/trash.svg alpaca_code/data/icons/paperclip.svg alpaca_code/data/icons/send.svg`

- [ ] **Step 3: Full verify**

Run: `python3 tests/test_selfcheck.py` → all PASS (`test_main_css_palette_tokens` needles: `#2f80ed` still present via `.alpaca-search:focus`/caret-color, `min-height: 30px` still present on editor tabs at main.py:111, `border-radius: 4px` on cards; `test_vector_icons_parse_and_paint` iterates whatever art remains).
Run (a display exists on this box): `python3 tests/responsive_probe.py` → assert lines pass, no-clip min-widths intact (Review Focus #2: tabs gain the deleted cluster's width; distributor maxima unchanged).
Run: `bin/alpaca-code <some project dir>` on screen once — visually: editor strip = tabs only, browser header = title + search, pane strip = tabs only, pane card ends after the notebook — then close. (Claude's console footer from circle 4a still shows when the console is idle+resumed — that is claude's own UI, expected.)

---

## Self-review notes

- Spec coverage: circles 1-3 removed in Tasks 1-3; circle 4 splits into (4a) claude-owned statusline — no possible app-side task, stated in Review Focus #1 — and (4b) ask row removed in Task 3 Step 2.
- Placeholders: none — every step carries the exact block to delete or the exact replacement.
- Type consistency: `_set_status(text, cls)` signature preserved (stubbed by tests/test_selfcheck.py:334/351/443); `on_status(text, cls)` contract unchanged for `window._on_run_status`; `_refresh_menus` keeps its name/callers (`register_actions`, `_on_pick`, `_act_new_project`).
- Review Focus items each land in a step above (no separate tests needed — suite already pins #3/#4, probe pins #2, greps pin #1/#5).

**No commits by the executor** — the tree already holds uncommitted prior work in the same files; tell the user what changed and let them commit.