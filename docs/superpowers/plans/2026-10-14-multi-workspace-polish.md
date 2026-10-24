# Multi-Workspace Header Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Polish the S4 header strip: floating hover band on project pills, bold/larger active pill, and the "+" rebuilt as a real icon button styled like the pill family.

**Architecture:** Three small fixes in `app.rs` + `assets/app.css`. The hover-band fix is the same measured law as `run_row` (app.rs:396-398): a `valign: FILL` widget stretches to the header's 32px interior, `Center` pins the band to its own requirement. The "+" swaps its text label for the already-shipped `plus.svg` stroke art through the existing `vector::icon()` path, and the hover rule becomes one grouped CSS rule shared by project pills, "+", and the status-bar branch pill.

**Tech Stack:** relm4 0.11 + gtk4 0.11, `vector.rs` Gsk paintables, USER-tier GTK CSS.

**Spec:** user request 2026-10-14 (this conversation, screenshots 8/9):
1. "When hovered, there should be gap between the top and the bottom. (reduce the margin between the text and the inside outline of the hover)" — the hover band (`#1c2431`) today fills the header's full interior and looks flush to the bar's top/bottom; the band must float with air above/below and hug the text (tighter text→band margin).
2. "The selected project needs more differentiation. Increase the selected projects text slightly and make the text bold."
3. "The '+' looks off. Additionally, its button styling should be the same as the projects and git branch button." Plus two investigation asks (findings in the next section).

## Investigation findings (carried so executors don't re-derive)

**How icons render here — two paths, the "+" takes path (a):**
- (a) `vector.rs` art icons: in-repo SVGs in `assets/icons/` → `vector::icon("name.svg")` → `VecIcon` (a `Gdk.Paintable` emitting Gsk fill/stroke nodes, sharp at any scale), consumed as `gtk::Image::from_paintable(...)` + optional `set_pixel_size(n)` (examples: `play.svg`/`stop.svg` at app.rs:325-352, `branch.svg`/`chevron.svg`/`plus.svg` in branchmenu.rs). Colors are BAKED into the SVG strokes; state recoloring is done with separate art (`x-dim.svg`) or css `opacity` — never CSS `color`.
- (b) seti pixbufs (badges.rs) — tree/tab/CHANGES cells only, never to be driven through vector.rs.
- `assets/icons/plus.svg` already exists (16-grid, stroke `#8a93a6` w1.5 round caps) and is already served by `vector::icon("plus.svg")` (branchmenu.rs:164). Zero new art.
- Why the current "+" reads off: `.label("+")` renders the font's x-height glyph (~7px in image #9 vs the 9px cap band of the pill text) — small and thin next to the pills, and its class has no `:hover` rule so it carries no hover band.

**Is a shared button class viable?**
Yes — but not as a new Rust-pushed class. The three pill buttons already paint the same shape (`transparent` bg, `border: none`, `radius: 6px`) and the identical hover band (`#1c2431` + `#e6e8ee`): `.alpaca-projbtn` (CSS:54-56), `.alpaca-projadd` (CSS:54 block + 60), `.alpaca-branchbtn` (CSS:159-167). The plan unifies them with ONE grouped hover rule (Task 3) so the family can't drift, keeping zero Rust churn. A Rust-side shared class (e.g. `append_css_class("alpaca-pillbtn")` on all three) was REJECTED: it touches 3 files, branchmenu's `set_css_classes` init, and buys nothing the grouped selector doesn't. Folding `.alpaca-branchbtn` into the line 49-53 zero-sweep was also rejected: the sweep zeroes `border-spacing`, whose Breeze value today feeds the measured status-bar row — regression risk with no visible payoff. The branch pill's label is a GRANDchild (`button > box > label`), so the family rule stays button-level; its colors/padding/backdrop label rules stay per-class (popover-pierce law, CSS:161-166).

**Measured anchors (user screenshots, both 1×):**
- Image #8: hover band = rows 41-72 exact `#1c2431` = 32px = the full header interior (flush top/bottom). Text ink rows 52-61 → text→band gap ≈ 7px. Root cause: pills default FILL, stretching to the header interior 32 — the exact bug class measured for `run_row` (app.rs:396-398: "default FILL would make the row allocate the header interior 32 — band would grow to 32").
- Image #9: header interior = 32 rows (21-52), hairline 53 = editor card border. "misiuscode" (active, brighter) vs "techshop-admin-frontend" (dim) vs x-height "+".
- After the fix the band = the strip's own requirement (label 18 + 2·2px pad ≈ 22-24, active 14px label ≈ 24-26) centred in the 32px bar → 3-5px air each side, text→band gap ≈ 2-3px. Target window for the probe assert: band in [20, 32).

## Global Constraints

- Test command: `cargo test --manifest-path alpaca-code-rs/Cargo.toml` (displayless, THE test command). `cargo build --manifest-path alpaca-code-rs/Cargo.toml` refreshes the runnable bin — cargo test does NOT.
- Probe gate: `bash /tmp/s4run.sh` (self-contained: rebuilds fixtures, seeds state.json with a GONE ghost, runs headless broadway, exits nonzero on any assert FAIL). Fresh broadway display number per run on socket errors.
- CSS/paint changes need a REAL restart: second spawn exits and forwards to the live app (single-instance). Kill the old process by exact pid, then relaunch — never a plain second spawn.
- No new dependencies; CSS edits only in `alpaca-code-rs/assets/app.css`; Rust edits only in `alpaca-code-rs/src/app.rs`.
- The S4 asserts are load-bearing: `alpaca-on` class names, `add_btn` object, strip re-append order [pill … pill | +] must survive unchanged.

## Review Focus

Focus classes the probe can't reach, most likely first — each pinned by the task noted:

1. **Hover on a pill while the window is unfocused (backdrop)** — band + light text must survive `:backdrop`; the pills' existing `label:backdrop` rules carry idle colors. Pinned by eyeballing a backdrop hover in Task 1's real-run step (branch-btn has run this way since 2026-10-06; no new rule expected).
2. **Cap-full state ("+" disabled)** — the icon must visibly grey out. CSS `opacity: 0.35` in Task 3; structurally pinned by the existing S4 `add-btn-disabled` assert.
3. **Three long project names with the active pill at 14px bold** — `max_width_chars(24)` + ellipsize must keep the strip inside the header (run pills must not be pushed). Pinned by eyeballing in Task 2's real-run step.
4. **Switch flicker** — bold/size change on switch moves strip width; expect instant restack, no residual band. Pinned by the existing S4 `on-class-wsA`/`two-open` asserts + eyeball.
5. **Stale allocation reads** — the new band assert runs 700ms after map; a `proj_box-h=0` print means never-allocated (the S4 hpane comment documents the race). Treat `0` as inconclusive → rerun, never tune CSS off a 0.

---

### Task 1: Floating hover band on the project pills

**Files:**
- Modify: `alpaca-code-rs/src/app.rs:359-361` (strip construction) and `app.rs:745-747` area (S4 step-2 asserts)
- Modify: `alpaca-code-rs/assets/app.css:54-56` (pill label ink law)

**Interfaces:**
- Consumes: `Workspace::new`'s `btn` construction (app.rs:228-231) — unchanged; `App::proj_box` (app.rs:359) — the strip Box, gets a valign.
- Produces: `pill-band-floats` S4 assert (uniform band across pills, strictly < 32). Task 3's grouped hover rule paints on the band this task floats.

**Why:** the pill buttons default `valign: FILL`; the headerbar offers the title strip the full 32px interior; every pill band stretches to 32 and reads flush with the bar's top/bottom (image #8). Same measured law as `run_row` — copied recipe, strip level. The label ink law (the `.alpaca-tabname` 3px fix) is included so the tighter band's ink stays optically centred: a centered label hangs ~1.3-1.8px low (measured twice on this build).

- [ ] **Step 1: Add the failing probe assert (TDD gate) in S4 step 2**

In `app.rs`, S4 `AppMsg::ProbeS4Step` step `2`, immediately after the `say("on-class-wsA", …)` block (before `title-is-basename`), insert:

```rust
// polish task: the pill band must FLOAT — stretched-to-the-bar is the
// image-#8 regression (default FILL); Center pins band = the strip's own
// req. Uniformity is free: all strip children share the strip's height.
// A bh of 0 = never-allocated (the S4 hpane comment's race) → inconclusive,
// rerun; never tune CSS off a 0.
let bh = self.proj_box.allocated_height();
let bands: Vec<i32> = self.ws.iter().map(|w| w.btn.allocated_height()).collect();
println!("PROBE strip: proj_box-h={} pill-bands={:?}", bh, bands);
let ok = bh > 0 && bands.len() == 2 && bands.iter().all(|h| *h == bh && *h < 32);
say("pill-band-floats", ok);
```

- [ ] **Step 2: Build and run the probe to verify it FAILS**

```bash
cargo build --manifest-path alpaca-code-rs/Cargo.toml && bash /tmp/s4run.sh 2 2>&1 | tail -20
```

Expected: `PROBE assert: pill-band-floats: FAIL` (today the band is stretched to 32; `proj_box-h` prints 32). Every other assert must PASS.

- [ ] **Step 3: Implement — strip valign + label ink law**

In `app.rs` (Workspace strip construction, currently at line 359):

```rust
let proj_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
```

becomes:

```rust
let proj_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
// the strip must NOT fill the bar: default FILL stretches every pill band to
// the header's 32px interior (image #8: band flush top/bottom). CENTER pins
// band = the strip's own req with air each side — the run_row recipe (band =
// the row it stands in, app.rs:396-398) applied at strip level.
proj_box.set_valign(gtk::Align::Center);
```

In `app.css`, directly after the `.alpaca-projbtn { … padding: 2px 10px; }` rule (line 54) — and BEFORE the `.alpaca-on` lines — add:

```css
/* ink law (the .alpaca-tabname fix): the eye anchors the dense ink band and a
   centered label hangs ~1.3-1.8px low — 3px bottom pad re-centers the ink in
   the pill band so the tight hover outline reads optically centred */
.alpaca-projbtn > label, .alpaca-projadd > label { padding: 0 0 3px 0; }
```

- [ ] **Step 4: Conditional fallback — only if the probe still reads 32**

If `proj_box-h` still prints 32 with the CENTER set (the headerbar's title slot force-filling the title widget), set valign on each pill button instead of / in addition to the strip — in `Workspace::new` after `btn.set_child(Some(&lbl))` (app.rs:236):

```rust
// per-button CENTER: bands ride each button's own req (the title slot
// force-fills — measured; per-band uniformity gives up ~2px of variation)
btn.set_valign(gtk::Align::Center);
```

Rerun the probe (Step 4 loop of Task 1: build + `bash /tmp/s4run.sh 2`); expected `pill-band-floats: PASS` with `proj_box-h` in [20, 32). Then re-apply the strip CENTER only if the assert fails on uniformity, never both silently: one of the two carries the fix, the other is belt-and-braces the probe decides for.

- [ ] **Step 5: Full unit suite + probe green**

(app.css is `include_str!`'d in style.rs and the probe runs the built binary: build before the probe.)

```bash
cargo test --manifest-path alpaca-code-rs/Cargo.toml \
  && cargo build --manifest-path alpaca-code-rs/Cargo.toml \
  && bash /tmp/s4run.sh 3 2>&1 | tail -20
```

Expected: all `cargo test` green; every `PROBE assert` line PASS (incl. `pill-band-floats`).

- [ ] **Step 6: Commit**

```bash
git add alpaca-code-rs/src/app.rs alpaca-code-rs/assets/app.css
git commit -m "mw header: float the pill hover band (FILL-stretch killed at strip level)

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 2: Active pill emphasis (bold, slightly larger)

**Files:**
- Modify: `alpaca-code-rs/assets/app.css:55`

**Interfaces:**
- Consumes: `alpaca-on` class toggling in `switch_to` (app.rs:1195-1202) — unchanged, already probed by `on-class-wsA`.
- Produces: nothing downstream; visual only. The strip band may gain ~2px of requirement height behind the bigger active label — already uniform and floating by Task 1, so no band-height coupling.

- [ ] **Step 1: Implement — bold + 14px on the active pill**

Line 55:

```css
.alpaca-projbtn.alpaca-on { color: #6b7487; font-size: 14px; font-weight: 700; }
```

(replaces the current `.alpaca-projbtn.alpaca-on { color: #6b7487; }`; font props ride the button node and inherit into the label — the branchbtn label-inherit law. `max_width_chars(24)` + ellipsize already cap the wider bold label; `label:backdrop` rules at lines 57-59 keep the idle colors.)

- [ ] **Step 2: Build + tests green**

(CSS is `include_str!`'d — rebuild so the probe binary carries it.)

```bash
cargo test --manifest-path alpaca-code-rs/Cargo.toml \
  && cargo build --manifest-path alpaca-code-rs/Cargo.toml \
  && bash /tmp/s4run.sh 2 2>&1 | tail -20
```

Expected: all green (incl. `pill-band-floats` — the band's new height is still uniform and < 32).

- [ ] **Step 3: Real-run eyeball (band, bold, long names)**

Kill the running app by exact pid, then:

```bash
cargo build --manifest-path alpaca-code-rs/Cargo.toml
alpaca-code-rs/target/debug/alpaca-code-rs &
```

Eyeball: active pill reads bold and a step larger; hover band floats (~4px air top/bottom); with three long-named projects open the strip stays inside the header (ellipsize cap). Report the strip's `proj_box-h`/`pill-bands` numbers from the last S4 log in the task report.

- [ ] **Step 4: Commit**

```bash
git add alpaca-code-rs/assets/app.css
git commit -m "mw header: active project pill bold + 14px

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 3: "+" becomes an icon button (+ shared-family hover rule)

**Files:**
- Modify: `alpaca-code-rs/src/app.rs:362-366` (add_btn build) and `app.rs` S4 step 2 (insert one assert after `pill-band-floats`)
- Modify: `alpaca-code-rs/assets/app.css:56` (hover rule → family rule), `app.css:60-61` (projadd disabled opacity)

**Interfaces:**
- Consumes: `vector::icon("plus.svg")` — already shipped and already consumed at branchmenu.rs:164, no registration needed. Task 1's floating band — the new hover band rides it.
- Produces: `add-btn-icon` S4 assert (the + carries an Image child, not a text label). The grouped hover rule is the shared-family deliverable of the investigation above.

- [ ] **Step 1: Add the probe assert**

In S4 step 2, directly after the `say("pill-band-floats", ok);` line:

```rust
// polish task: the "+" lost its text label for the plus.svg stroke icon
// (image #9: a text "+" is an x-height glyph, ~7px next to 9px cap text)
say(
    "add-btn-icon",
    self.add_btn.child().and_downcast::<gtk::Image>().is_some(),
);
```

- [ ] **Step 2: Build and run the probe to verify it FAILS**

```bash
cargo build --manifest-path alpaca-code-rs/Cargo.toml && bash /tmp/s4run.sh 2 2>&1 | tail -20
```

Expected: `PROBE assert: add-btn-icon: FAIL`.

- [ ] **Step 3: Implement the icon button**

In `app.rs` (the strip assembly, currently lines 362-366):

```rust
let add_btn = gtk::Button::builder()
    .has_frame(false)
    .css_classes(["alpaca-projadd"])
    .label("+")
    .build();
```

becomes:

```rust
// the "+" rides plus.svg's stroke art: a text "+" is an x-height glyph
// (measured image #9, ~7px vs the pill text's 9px cap band) and carried no
// hover band. Same recipe as the run icons (paintable + pixel_size + valign);
// 14px matches the 13px pill text; the baked #8a93a6 stroke matches the
// branch-icon family (branch.svg stays grey on hover too — baked-stroke law:
// recolor is opacity/alt-art only, CSS color can't reach a Gsk node).
let add_btn = gtk::Button::builder()
    .has_frame(false)
    .css_classes(["alpaca-projadd"])
    .build();
if let Some(vec) = vector::icon("plus.svg") {
    let img = gtk::Image::from_paintable(Some(&vec));
    img.set_pixel_size(14);
    img.set_valign(gtk::Align::Center);
    add_btn.set_child(Some(&img));
} else {
    // art loss (never on a shipped build): the old text stand-in
    add_btn.set_child(Some(&gtk::Label::builder().label("+").build()));
}
```

The existing `add_btn.connect_clicked` / tooltip / re-append ordering lines stay untouched.

- [ ] **Step 4: Implement the CSS (family hover rule + disabled opacity)**

Replace line 56 (.alpaca-projbtn hover) with the family rule:

```css
/* pill-family hover — ONE band for the whole family: header project pills,
   header "+", status-bar branch pill (branchmenu.rs paints the same values
   by hand today). Grouped so the three can never drift again; band paints on
   hover and on the branch pill's pinned-open state (alpaca-open). */
.alpaca-projbtn:hover, .alpaca-projbtn.alpaca-on:hover, .alpaca-projadd:hover,
.alpaca-branchbtn:hover, .alpaca-branchbtn.alpaca-open {
    background: #1c2431; color: #e6e8ee;
}
```

Replace line 61:

```css
.alpaca-projadd:disabled { color: #3d4454; }
```

with:

```css
/* the icon child can't take `color` (baked Gsk strokes): disabled rides
   opacity — the runpill disabled law (CSS line ~97) */
.alpaca-projadd:disabled { color: #3d4454; opacity: 0.35; }
```

Then delete the now-duplicated `.alpaca-branchbtn:hover, .alpaca-branchbtn.alpaca-open { … }` rule (line 167), keeping its surrounding comment and the `label:backdrop` rule (line 168) — the family rule above already carries those selectors and sits EARLIER in the file, so nothing later re-sets the branch hover (verified: no other `.alpaca-branchbtn:hover` rule exists).

- [ ] **Step 5: Full suite + probe green**

```bash
cargo test --manifest-path alpaca-code-rs/Cargo.toml \
  && cargo build --manifest-path alpaca-code-rs/Cargo.toml \
  && bash /tmp/s4run.sh 2 2>&1 | tail -20
```

Expected: all green, including `add-btn-icon: PASS` and the existing `add-btn-disabled: PASS`.

- [ ] **Step 6: Real-run eyeball**

Same restart procedure as Task 2 Step 3. Eyeball: "+" reads as a button sibling of the pills (icon size/weight against the text), hover band matches the pills' hover, and at the cap the icon greys out (opacity).

- [ ] **Step 7: Commit**

```bash
git add alpaca-code-rs/src/app.rs alpaca-code-rs/assets/app.css
git commit -m "mw header: '+' rides plus.svg + one grouped pill-family hover rule

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

## Post-plan note for the user

Restart law applies (CSS change): kill the old app process by exact pid, relaunch the binary — a second spawn only forwards to the running instance and shows the OLD css.