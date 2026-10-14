# Workbench Menu Header Swap Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the File/Edit/Window/Help row in the headerbar with one small down-arrow menu button whose popover holds New Workbench / Open Workbench / Save Workbench / Exit (thin separators between the three groups); the three workbench items are no-op stubs until workbenches are implemented.

**Architecture:** The menubar is four custom label-mode `gtk::MenuButton`s packed `pack_start` into the header (app.rs:394-397), populated by `refresh_menus` re-attaching `PopoverMenu`s built from `file_menu`/`edit_menu`/`win_menu`/`help_menu` (app.rs:1329-1386). All of that dies and becomes ONE icon-mode `MenuButton` (child = `chevron-down.svg` via the existing `vector::icon()` path), whose static `gio::Menu` uses `append_section` — sections are what `PopoverMenu` renders as separator lines. `Exit` reuses the existing `app.quit` action (Ctrl+Q accel already registered). Three new no-op `SimpleAction`s make the workbench rows look enabled without doing anything. The existing `.alpaca-header menubutton > button` CSS rule (app.css:73-76) already styles the new button — zero CSS changes.

**Tech Stack:** relm4 0.11 + gtk4 0.11, `vector.rs` Gsk paintables, gio `MenuModel` sections, USER-tier CSS (untouched).

**Spec:** user request 2026-10-14 (this conversation, image #12 — a crop of the current File Edit Window Help strip):
1. Investigate how icons are handled (findings carried below); then remove "File", "Edit", "Window", "Help".
2. "Instead, at the left there should be a small arrow pointing down. Pressing on it should show 'New Worbench', 'Open Workbench', 'Save Workbench', 'Exit'" (labels used in this plan: "New Workbench", "Open Workbench", "Save Workbench", "Exit" — the user's "Worbench" reads as "Workbench").
3. Separators: New Workbench + Open Workbench | thin line | Save Workbench | thin line | Exit.
4. "Currently those should do nothing as Workbenches are not yet implemented (they will be essentially like superprojects — projects that load the up-to 3 projects … ignore them for now)."

## Investigation findings (carried so executors don't re-derive)

**How the current menubar is built:** NOT a GTK `MenuBar`/`ApplicationWindow.set_show_menubar` — four bare label-mode `gtk::MenuButton`s (label "File"/"Edit"/"Window"/"Help"), arrow widget hidden by `no_arrow()` (app.rs:1308-1327, the 16×24 arrow-walk law), packed `pack_start` in label order (first appended = leftmost). Popovers are reattached in `refresh_menus` — which is called exactly once, at init end (app.rs:459). There is no other caller.

**How icons render here — two paths, the arrow takes path (a):**
- (a) `vector.rs` art icons: in-repo SVGs in `assets/icons/` → `vector::icon("name.svg")` → `VecIcon` (Gdk.Paintable emitting Gsk fill/stroke nodes — sharp at any pixel size), consumed as `gtk::Image::from_paintable(...)` + `set_pixel_size(n)` (run/stop/plus at app.rs:325-387, `chevron-down.svg` at branchmenu.rs:183). Colors are BAKED into the SVG strokes (`#8A93A6` grey, the branch-art family); CSS `color` cannot reach a Gsk node.
- (b) seti pixbufs (badges.rs) — tree/tab/CHANGES cells only, never driven through vector.rs.
- `assets/icons/chevron-down.svg` already exists: 16-grid viewBox, `stroke="#8A93A6" stroke-width="2"` on integer centerlines `M4 6L8 10L12 6` — exactly "a small arrow pointing down", already served by `vector::icon("chevron-down.svg")` (golden count test pins it in vector.rs:439). Zero new art.
- Theme-symbolic icon names were measured to grey out on this build — that's why art is baked SVG.

**MenuButton arrow law (measured, CLAUDE.md):** label-mode MenuButtons draw a real 16×24 `down` arrow widget; `set_always_show_arrow(False)` is a no-op on this build; icon-mode buttons hide the arrow themselves. The new button uses a custom child (an image), so NO `no_arrow()` call — and since the four label buttons die, `no_arrow()` and the walking law lose their last callers, **`no_arrow` is deleted** (Task 2). If the new icon button ever showed a stray arrow in Task 3's visual check, that would falsify "icon/custom-child hides the arrow" — fix then, not speculatively.

**Separators:** `gio::Menu::append_section` between groups → `PopoverMenu` renders a separator line between sections. That is the entire "thin line" mechanism — no CSS, no `gio::Menu::append(Some("—"), …)` hack.

**Header styling:** app.css has `.alpaca-header menubutton > button` (lines 73-76: transparent bg, radius 6, `padding: 0 8px`, `margin: 3px 0` → 28px paint pill, hover `#111723`). The new MenuButton rides it unchanged. The old rule's "34px label floor" note (comment above it) described the label row; the icon button is shorter (~16-18px button, 28px paint span) — that is fine, no CSS edit, but Task 3 eyeballs the ink centering (the image needs `valign: CENTER`, the run-icon law measured at app.rs:334).

**Action wiring:** `register_actions` (app.rs:1477-1569) registers `open-project`, `new-project`, `quit` (accel Ctrl+Q), the edit-op loop `undo/redo/cut/copy/paste/select-all` → `AppMsg::EditOp`, `win-minimize`, `close`, `about`, `open-recent`.

**Scoping decision — what is deleted vs kept dormant:**
- DELETED: the four buttons, the four menu-model fns, `refresh_menus` (both), `HeaderButtons` struct (App field included), `no_arrow`, and the three model tests (`file_menu_items_and_recents_submenu_model`, `file_menu_without_recents_has_no_submenu`, `edit_window_help_menus_populated`). Their UI is gone; the tests test deleted fns.
- KEPT DORMANT (deliberate, marked with a one-line comment in Task 1): `open-project`, `new-project`, `quit`, the edit-op actions, `win-minimize`, `close`, `about`, `open-recent` + `AppMsg::RecentMissing` stay registered with no menu surface. Workbenches (aka superprojects) are the very next feature and will restructure these flows with their own menus; deleting wiring that an imminent redesign will rewrite in its own shape is churn, and undo/redo/clipboard routing (`AppMsg::EditOp` → `EditorMsg` → editor.rs handlers) is real machinery that must not be casually amputated. The workbench phase deletes whatever it does not re-surface.
- ADDED: `new-workbench`/`open-workbench`/`save-workbench` no-op stub actions + `workbench_menu()` + one MenuButton. Why stub actions instead of action-less menu items: items with no action attribute render insensitive/greyed in a PopoverMenu — the spec wants rows that LOOK enabled and just do nothing, and these three become the wiring slots when workbenches land.

**Probe safety (checked):** the S2/S3/S4 probe chains do not reference menu buttons or the menu models (grep clean); no assert counts `pack_start` children. `state.json` is untouched by this change.

## Global Constraints

- Test command: `cargo test --manifest-path alpaca-code-rs/Cargo.toml` (displayless, THE test command). `cargo build --manifest-path alpaca-code-rs/Cargo.toml` refreshes the runnable bin — cargo test does NOT. Run before every manual launch.
- Single-instance law: a second spawn exits and forwards to the running primary. To see changes, kill the old process by exact resolved pid (`pgrep -x alpaca-code-rs`, then `kill <pid>s`), never `pkill -f` (kills the invoking shell's process group — measured exit 144), then launch fresh.
- Rust edits ONLY in `alpaca-code-rs/src/app.rs` (+ the CLAUDE.md status note in Task 3). CSS: NONE (existing header-menubutton rule covers the new button).
- Menu labels EXACTLY: "New Workbench", "Open Workbench", "Save Workbench", "Exit".
- New actions named `new-workbench`, `open-workbench`, `save-workbench` → items reference `app.new-workbench`, `app.open-workbench`, `app.save-workbench`; Exit → existing `app.quit`.
- No new dependencies.
- Line numbers below are current at branch HEAD (aee23a1, clean tree). Edits drift them — anchor by the quoted code content, not bare numbers.
- Commit messages end with `Co-Authored-By: Claude Code <noreply@anthropic.com>`.

## Review Focus

Failure modes the spec implies that no unit test reaches (the model test only pins labels/actions/separators), each pinned by Task 3's real-run checklist:

1. **No-op clicks** — New/Open/Save Workbench clicked: popover must close, app must stay sane (never crash on the empty `connect_activate`); state.json must not gain workbench garbage. Pinned by Task 3 step 4.
2. **Stale/popover re-attach regression** — the old code reattached popovers every session start; the new one attaches ONCE: open the menu twice, expect the same four rows both times. Pinned by Task 3 step 3.
3. **Arrow ink centering** — 14px image in the `margin: 3px 0` menubutton pill; the run-icon law says default align lands the ink top — `set_valign(Center)` is in the Task 2 code; if the chevron still rides high/low in a 28px paint band, measure before CSSing. Pinned by Task 3 step 2.
4. **Header geometry** — removing the four 34px-floor label buttons must not change the window's minimum size (that floor is the title-widget chain, per CLAUDE.md) nor shift the centered pill strip / run pills. Pinned by Task 3 step 5.
5. **Art loss fallback** — `vector::icon("chevron-down.svg")` returns None (only on a broken asset build) → the `else` stand-in (icon-name menu button) must still open the workbench menu. Can't be displayless-tested (GTK init); pinned by code-reading in Task 2 review.

---

### Task 1: `workbench_menu()` model + three no-op stub actions (TDD)

**Files:**
- Modify: `alpaca-code-rs/src/app.rs` (menu-model fns area ~1338; `register_actions` ~1477; tests module ~1582)

**Interfaces:**
- Consumes: existing `app.quit` action (app.rs:1498-1503 + Ctrl+Q accel 1568); test helper `attr()` (app.rs:1613-1619, lives in the tests module).
- Produces: `workbench_menu() -> gio::Menu` (Task 2's init attaches it); actions `app.new-workbench`, `app.open-workbench`, `app.save-workbench` registered (the workbench phase's wiring slots).

- [ ] **Step 1: Write the failing test** — inside the existing `#[cfg(test)] mod tests` (app.rs ~1582), beside `edit_window_help_menus_populated`:

```rust
#[test]
fn workbench_menu_sections_labels_actions() {
    use gio::prelude::MenuModelExt;
    // 3 sections = the two separator lines: [New, Open] | [Save] | [Exit]
    let m = workbench_menu();
    assert_eq!(m.n_items(), 3);
    let sec = m.item_link(0, gio::MENU_LINK_SECTION).expect("section 0");
    assert_eq!(sec.n_items(), 2);
    assert_eq!(attr(&sec, "label", 0).as_deref(), Some("New Workbench"));
    assert_eq!(attr(&sec, "action", 0).as_deref(), Some("app.new-workbench"));
    assert_eq!(attr(&sec, "label", 1).as_deref(), Some("Open Workbench"));
    assert_eq!(attr(&sec, "action", 1).as_deref(), Some("app.open-workbench"));
    let sec = m.item_link(1, gio::MENU_LINK_SECTION).expect("section 1");
    assert_eq!(sec.n_items(), 1);
    assert_eq!(attr(&sec, "label", 0).as_deref(), Some("Save Workbench"));
    assert_eq!(attr(&sec, "action", 0).as_deref(), Some("app.save-workbench"));
    let sec = m.item_link(2, gio::MENU_LINK_SECTION).expect("section 2");
    assert_eq!(sec.n_items(), 1);
    assert_eq!(attr(&sec, "label", 0).as_deref(), Some("Exit"));
    assert_eq!(attr(&sec, "action", 0).as_deref(), Some("app.quit"));
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test --manifest-path alpaca-code-rs/Cargo.toml workbench` 
Expected: FAIL to compile — `workbench_menu` not defined.

- [ ] **Step 3: Implement** — add the model fn beside the old menu fns (after `edit_menu`, app.rs ~1357) and the stub actions in `register_actions` (right after the `quit` block, app.rs ~1503):

```rust
/// The one header menu: workbench actions. Sections between groups are
/// what PopoverMenu renders as the separator lines — no separator rows.
fn workbench_menu() -> gio::Menu {
    let m = gio::Menu::new();
    let sec = gio::Menu::new();
    sec.append(Some("New Workbench"), Some("app.new-workbench"));
    sec.append(Some("Open Workbench"), Some("app.open-workbench"));
    m.append_section(None, &sec);
    let sec = gio::Menu::new();
    sec.append(Some("Save Workbench"), Some("app.save-workbench"));
    m.append_section(None, &sec);
    let sec = gio::Menu::new();
    sec.append(Some("Exit"), Some("app.quit"));
    m.append_section(None, &sec);
    m
}
```

```rust
    // no-op stubs until workbenches land (they'll be the superproject layer
    // loading the up-to-3 projects): the rows must look enabled and do
    // nothing yet — these three become the wiring slots then.
    for name in ["new-workbench", "open-workbench", "save-workbench"] {
        let a = gio::SimpleAction::new(name, None);
        a.connect_activate(|_a, _p| {});
        app.add_action(&a);
    }

    // Surface-less for now (File/Edit/Window/Help rows removed 2026-10-14):
    // open-project/new-project/undo/redo/cut/copy/paste/select-all/
    // win-minimize/close/about/open-recent keep firing — the workbench
    // phase re-surfaces or deletes them when it defines its own menus.
```

(The second comment is a plain marker comment above the remaining registrations — the first comment block goes above the stub loop.)

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --manifest-path alpaca-code-rs/Cargo.toml` 
Expected: PASS everywhere (old menu tests still green — the old fns still exist).

- [ ] **Step 5: Commit**

```bash
git add alpaca-code-rs/src/app.rs
git commit -m "menu: add workbench menu model + no-op stub actions

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 2: Header swap — one chevron MenuButton, old row deleted

**Files:**
- Modify: `alpaca-code-rs/src/app.rs` (init ~304-459; structs ~97-103, ~275; helpers 1273-1336; menu fns 1338-1386; tests 1621-1678)

**Interfaces:**
- Consumes: `workbench_menu()` + the three stub actions from Task 1; `vector::icon("chevron-down.svg")`; the existing css rule `.alpaca-header menubutton > button`.
- Produces: the app's only menu surface — one `MenuButton` packed first on the header's left. Nothing else consumes it (static popover, attach-once), so it needs NO App field: init packs it into `header`, which owns it.

- [ ] **Step 1: Write the init replacement** — delete the `menu_btns` construction block (app.rs:306-319) and the four `pack_start` lines + their comment (393-397), and in their place, after the `add_btn` block ends (just before `header.set_title_widget`, app.rs:392 — same order the old code used: menu row after the title strip):

```rust
        // The one header menu: a small down arrow (chevron-down.svg — 16-grid,
        // w2 #8A93A6, the branch-art family) opening the workbench actions.
        // Custom-child = icon mode: MenuButton hides its own arrow (measured
        // law), so no arrow walk. Header menubuttons are styled by the bare
        // `.alpaca-header menubutton > button` css rule. img valign CENTER rides
        // the run-icon law (default FILL parks ink high in the taller button).
        let menu_btn = gtk::MenuButton::new();
        if let Some(vec) = vector::icon("chevron-down.svg") {
            let img = gtk::Image::from_paintable(Some(&vec));
            img.set_pixel_size(14);
            img.set_valign(gtk::Align::Center);
            menu_btn.set_child(Some(&img));
        } else {
            // art loss (never on a shipped build): symbolic stand-in
            menu_btn.set_child(Some(&gtk::Image::builder()
                .icon_name("pan-down-symbolic")
                .pixel_size(14)
                .valign(gtk::Align::Center)
                .build()));
        }
        menu_btn.set_tooltip_text(Some("Workbench"));
        menu_btn.set_popover(Some(&gtk::PopoverMenu::from_model(Some(&workbench_menu()))));
        header.pack_start(&menu_btn);
```

- [ ] **Step 2: Delete the now-dead plumbing** — remove each block fully (anchor by content):
  1. `struct HeaderButtons { file: gtk::MenuButton, … }` (app.rs:97-103).
  2. the `menu_btns: HeaderButtons,` field in `App` (~275) and the `, menu_btns,` line in the `model = App { … }` literal (~450).
  3. `model.refresh_menus();` (~459) — the workbench popover was attached in init already.
  4. the `refresh_menus` method (app.rs:1273-1275).
  5. `no_arrow` (app.rs:1308-1327) — its only callers were the deleted label buttons.
  6. the free `refresh_menus` (app.rs:1329-1336).
  7. `file_menu` / `edit_menu` / `win_menu` / `help_menu` (app.rs:1338-1386) — `open_project`/`new_project`/`error_dialog` BELOW them STAY (the `+` button and ProjectCreated flow use them).
  8. the tests `file_menu_items_and_recents_submenu_model` (~1621-1648), `file_menu_without_recents_has_no_submenu` (~1650-1662), `edit_window_help_menus_populated` (~1664-1678). KEEP the `attr` helper — `workbench_menu_sections_labels_actions` uses it. (It still imports `state::State`/`fixture`? `fixture` was only for the file_menu tests — delete `fixture` too; keep `add_decision`-adjacent code and `State` import, which `fixture` alone used — delete both.)

  After deletion, `use crate::state::State;` inside the tests module becomes unused — drop it with `fixture`.

- [ ] **Step 3: Run the tests**

Run: `cargo test --manifest-path alpaca-code-rs/Cargo.toml` 
Expected: PASS everywhere — including `workbench_menu_sections_labels_actions`.

- [ ] **Step 4: Build the runnable bin**

Run: `cargo build --manifest-path alpaca-code-rs/Cargo.toml` 
Expected: builds clean (warnings about nothing — the deleted pieces are gone together).

- [ ] **Step 5: Commit**

```bash
git add alpaca-code-rs/src/app.rs
git commit -m "menu: swap File/Edit/Window/Help row for one workbench chevron menu

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 3: Real-run verification + CLAUDE.md status note

**Files:**
- Modify: `alpaca-code-rs/CLAUDE.md` (Status section + the menubar gotcha bullets)

- [ ] **Step 1: Build + kill the running instance + launch** (single-instance law; never a plain second spawn):

```bash
cargo build --manifest-path alpaca-code-rs/Cargo.toml
pgrep -x alpaca-code-rs   # resolve pids
kill <pids from pgrep>    # by exact pid, never pkill -f
/home/dominykasm/FunProjects/misiuscode/alpaca-code-rs/target/debug/alpaca-code-rs &
```

- [ ] **Step 2: Header eyeball** — one small down chevron at the header's LEFT; ink vertically centered in the menubutton hover band (28px paint span); pills still centered; run pills on the right unchanged.

- [ ] **Step 3: Menu content** — click the chevron TWICE: four rows both times, no greyed items, two thin separators: New/Open above line 1, Save above line 2, Exit last.

- [ ] **Step 4: Behavior** — click New/Open/Save Workbench: menu closes, nothing changes, no panic (nothing in stdout but the normal VTE output). Click Exit → app quits. Relaunch, press Ctrl+Q → quits.

- [ ] **Step 5: Everything else intact** — + button opens a project; tabs/run pills/Terminal/Output panes unaffected (this change touches only the header row).

- [ ] **Step 6: Update CLAUDE.md** — smallest truthful touches:
  - Status section: prepended line `Header menu (2026-10-14): the File/Edit/Window/Help row is GONE — one icon-mode chevron MenuButton (`chevron-down.svg`, custom child) opens the workbench menu (New/Open | Save | Exit sections = separators; new/open/save = no-op stub actions, Exit = app.quit). Static popover attached once at init — refresh_menus is deleted.` (two sentences, matching house style).
  - The "Label-mode GtkMenuButtons…" gotcha bullet: append `(2026-10-14: currently no label-mode MenuButton survives — the header menu button is icon mode, whose arrow IS hidden for you; the walk law is kept for future label menus.)` — do not rewrite any other gotcha.

- [ ] **Step 7: Commit**

```bash
git add alpaca-code-rs/CLAUDE.md
git commit -m "docs: CLAUDE.md status for the workbench header menu

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```