import os
import alpaca_code.gi_env as ge
ge.require("Gdk", ("4.0",))
ge.require("Gtk", ("4.0",))
ge.require("GtkSource", ("5",))
from gi.repository import Gdk, GLib, Gtk, GtkSource
from . import window

CSS = """
/* the one hover fill: menubar pill, run-cell bands, tab hovers, tree-row band
   (filetree reads this token per paint via StyleContext.lookup_color) */
@define-color alpaca-hover #111723;
/* Mockup "Agentic App – Dark IDE" (measured numbers, window-relative px):
   near-black gutter (#07090d) carries floating cards (#0d1017, radius 4 = the
   WM's window radius); strips inside cards are darker (#0a0d13); raised pills
   (#111723) sit on strips with a #2b3448 hairline. */
/* design: the menubar zone touches the window top edge; the gutter under it is
   side/bottom 6px only — the design's header rule and 8px top gap were cut
   2026-10-04 (user's circled artifacts: cards now sit flush under the menus row). */
/* Breeze also fills the window node via .background:backdrop (#202326) — the
   stateless app rule loses on :backdrop, so restate it (same mechanism as the
   notebook strip): otherwise an unfocused window turns every exposed strip of
   window bg Breeze-gray. */
window, window:backdrop { background: #07090d; padding: 0 6px 6px; }
.alpaca-card { background: #0d1017; border: 1px solid #1c2230; border-radius: 4px; }

paned > separator { background: transparent; min-width: 6px; min-height: 6px; }
/* both gutters stay the shared 6px air (2026-10-05, user reversed the flush
   editor↔console junction): the vertical separator needs no override, and the
   drag knob is grabbable again — positions (window.py) remain the defaults. */

/* --- header: menus row, flush top, no rules --- (2026-10-04, user's circled
   artifacts): Breeze paints headerbar { border-top: 1px solid
   lighter(@theme_titlebar_background_breeze) } — lighter(#272c31) = #343940,
   measured — and clips it short of both corners (headerbar border-radius
   5px 5px 0 0 inside the window's 6px padding) = the "bar not reaching the
   corners"; headerbar:backdrop refills the bar #202428. The stateless app rule
   loses on :backdrop, so the app values are restated there — same mechanism as
   the notebook strip below.
   Titlebuttons must NOT get `background` — the theme paints their icons via
   background-image; a transparent background erases them. Breeze inflates
   ~46px outer per titlebutton (min 18 + 28px padding + -14px margins, measured)
   which alone pushes the bar to ~52 — collapse them to sane metrics. */
.alpaca-header, .alpaca-header:backdrop { background: transparent; min-height: 32px;
                                          padding: 0 6px; border-top: none;
                                          border-bottom: none; }
headerbar.alpaca-header windowcontrols { padding: 0; margin: 0; }
headerbar.alpaca-header windowcontrols button { min-width: 24px; min-height: 22px;
                                                padding: 0; margin: 0; }
.alpaca-wintitle { color: #6b7487; font-weight: 500; font-size: 13px; }
/* menu buttons floor at 34px on this build/theme (GtkImage 16px floor + 18px
   13px label, measured) — zero vertical padding keeps them from riding higher.
   Hover pill must grow WITHOUT growing the bar: pill paint = wrapper(34) − child
   v-margins (wrapper keeps a 16px child v-margin: paint 34−2·8 = 18, measured) —
   shrinking the child margin grew the paint 18→22 (margin 6) →28 (margin 3),
   matching the runpill hover band height; min untouched. */
.alpaca-header menubutton > button { background: transparent; color: #e6e8ee; font-size: 13px;
                                     border: none; border-radius: 6px; padding: 0 8px;
                                     min-height: 2px; margin: 3px 0; }
.alpaca-header menubutton > button:hover { background: #111723; }
/* Run/Stop: flat idle like the menu buttons (user: the idle raised pill read as
   wrong fill) — the shell exists only while a run is live (class toggled by
   window._refresh_run_style). No border in either state, so the running flip
   never jumps layout. The cells are 28px tall (runpill min-height): the
   buttons themselves refuse to shrink vertically at APPLICATION priority
   (min-height 2px / padding 0 measured 32px button mins, probe_pill9-16) —
   USER priority moves them (probe_mtx: px-true 12/13/14/16 tall, minh28 → 28).
   Hover swatches live on the wrapper cells, NOT the buttons: cells paint
   their full (fill) allocation, so a rounded background-color on
   .alpaca-runcell:hover is a rounded 24/22 × 28 band flush with the menu
   pills' height. */
.alpaca-runpill { background: transparent; border: none; border-radius: 8px;
                  min-height: 28px; }
.alpaca-runpill.running, .alpaca-runpill.running:hover { background: #111723; }
/* Zero the button's OWN css metrics first (min-height/padding/border on a
   headerbar-ancestry button are lost to a ~16px vertical additive that
   survives every button-side kill — probe_dis: headerbar button = 28 css + 16
   = 44; everything-0 incl. the child sweep drops it to 14x14), then re-raise
   the wanted 28x22 in the later rule (last equal-priority rule wins). */
.alpaca-runpill button, .alpaca-runpill button > box,
.alpaca-runpill button > box > image {
    min-height: 0px; min-width: 0px; padding: 0px; margin: 0px; border: none;
    border-spacing: 0px; }
.alpaca-runpill button { background: transparent; border: none;
                         min-height: 28px; padding: 0 4px; }
.alpaca-runpill button:disabled { opacity: 0.35; }
/* the hover paint is a real background-color at the cell's full allocation —
   radius-6 clipped by the paint itself, so the corners are ROUND (the
   background-image band this replaced could only paint square or arc-bitten
   corners: bg-image is clipped to the cell's radius path — full-width = bite,
   inset = square, measured both ways). */
.alpaca-runcell { border-radius: 6px; }
.alpaca-runcell:hover { background-color: #111723; }
.alpaca-runpill.running .alpaca-runcell:hover { background-color: #1b2233; }
.alpaca-rundivider { min-width: 1px; background: transparent; margin: 4px 0; }
.alpaca-runpill.running .alpaca-rundivider { background: #2b3448; }

/* --- file browser --- */
.alpaca-panel-title { font-size: 16px; font-weight: 600; color: #e6e8ee; }
.alpaca-search { background: #0a0d13; color: #e6e8ee; font-size: 13px;
                 border: 1px solid #2b3448; border-radius: 8px;
                 min-height: 10px; padding: 0 10px; margin-top: 0; margin-bottom: 6px;
                 caret-color: #2f80ed; }   /* height comes from the 30px size_request
                                              (see filetree) — entry padding only fights it */
.alpaca-search:focus { border-color: #2f80ed; }
.alpaca-search image { margin-right: 6px; }
/* workspace/changes mode buttons (spec §1): text-only section-label type —
   no frame fill; hover brightens, the selected mode's `alpaca-on` stays
   bright. min-height/padding sweep keeps Breeze's 32px button floor off a
   13px-ink label (app css is USER priority — wins).
   Ref's blue underline: every tab reserves a static 2px transparent
   border-bottom (toggling the class moves zero geometry), the active one
   paints it #2f80ed; the strip-wide 1px hairline is the tabs box's own
   border-bottom, so the underline sits flush on it. Button min-height
   grows by the reserved 2px: CSS min-height is the border box — the ink
   room above must not shrink with the class toggle. */
.alpaca-modetabs { padding: 0 12px; border-bottom: 1px solid #1c2230; }
.alpaca-tabbtn { color: #8a93a6; font-size: 11px; font-weight: 500;
                 letter-spacing: 1px; min-height: 18px; padding: 0;
                 background: transparent; border: none;
                 border-bottom: 2px solid transparent; border-radius: 0;
                 box-shadow: none; outline: none; }
.alpaca-tabbtn:hover { color: #e6e8ee; }
.alpaca-tabbtn.alpaca-on { color: #e6e8ee; border-bottom-color: #2f80ed; }
/* backdrop: the tab color is inherited from the button — Breeze's direct
   label:backdrop rule (#fcfcfc) beat it at any provider priority, brightening
   BOTH tabs in an unfocused window (read as "both selected"). Restate per
   child label node (direct-vs-direct now); :focus measured exonerated. */
.alpaca-tabbtn label:backdrop { color: #8a93a6; }
.alpaca-tabbtn.alpaca-on label:backdrop { color: #e6e8ee; }
.alpaca-tree { background: transparent; color: #e6e8ee; font-size: 13px; }
/* entry floors ~36px on this theme even with min-height swept to 0
   (measured) — 14px keeps the text node from riding the font size */
.alpaca-search text { min-height: 14px; padding: 0; }
.alpaca-tree.view:selected { background: #1b3560; color: #ffffff; }
.alpaca-tree:selected { background: #1b3560; color: #ffffff; }

/* --- bottom status bar (browser): ~22px git row over a 1px rule --- */
/* horizontal padding 0: the bar's own 12px margins ARE the inset — matching the
   commit bar's edge exactly (an extra 10px here read as the misaligned row) */
.alpaca-statusbar { min-height: 0; padding: 2px 0; border-top: 1px solid #1c2230;
                    color: #8a93a6; font-size: 12px; }
.alpaca-statusbar label { color: #8a93a6; font-size: 12px; }
.alpaca-statusbar > label:last-child { color: #5a6375; }

/* --- branch pill v2 (2026-10-06, user mockup): DEFAULT IS THE PLAIN STATUS ROW
   (revert) — the raised pill is hover/open only; no ▾ arrow in any state --- */
.alpaca-branchbtn { background: transparent; border: none; border-radius: 6px;
                    min-height: 0; padding: 1px 8px; color: #8a93a6; }
/* open/hover text color rides ON THE BUTTON: label-inherit. The old
   `branchbtn.alpaca-open label` descendant selectors pierced the popovers
   (set_parent makes them css children of the pill) and out-specified
   .alpaca-hint/.alpaca-branchitem at the same USER tier — hint rendered
   #e6e8ee bright (measured 227,229,235). Every popover label carries its own
   explicit rule → inherits can't reach them, only the pill's own strip label. */
.alpaca-branchbtn:hover, .alpaca-branchbtn.alpaca-open { background: #1c2431; color: #e6e8ee; }
.alpaca-branchbtn label:backdrop { color: #8a93a6; }   /* tabs' label:backdrop leak law */
/* popovers: Breeze paints popover > contents (bg + #4c4e51 border + tail) —
   styling the popover node too paints a double frame (measured 17,23,35 ring
   around 28,31,34); so the popovers go bare and only rows/items carry css */
.alpaca-branchitem { background: transparent; border: none; border-radius: 6px;
                     min-height: 0; padding: 4px 10px; color: #8a93a6; font-size: 13px; }
.alpaca-branchitem:hover { background: #1c2230; color: #e6e8ee; }
.alpaca-branchitem.alpaca-on { color: #e6e8ee; }
.alpaca-branchitem label:backdrop { color: #8a93a6; }
.alpaca-branchitem.alpaca-on label:backdrop { color: #e6e8ee; }
.alpaca-hint { color: #8a93a6; font-size: 12px; font-style: italic; }

/* --- changes view: commit bar (spec §6) --- */
/* no border-top: the bar's 6px top margin air is the separator — a 1px rule
   here paints a stray hairline directly above the message input */
.alpaca-msg { background: #0a0d13; color: #e6e8ee; font-size: 13px;
              border: 1px solid #2b3448; border-radius: 8px;
              min-height: 10px; padding: 0 12px; }
.alpaca-msg:focus { border-color: #2f80ed; }
.alpaca-msg text { min-height: 14px; padding: 0; }
.alpaca-barbtn { background: #111723; color: #e6e8ee; border: 1px solid #2b3448;
                 border-radius: 8px; font-size: 12px; font-weight: 500;
                 padding: 0 12px; min-height: 26px; }
.alpaca-barbtn:hover { background: #1c2431; }
.alpaca-barbtn:disabled { color: #5a6375; background: #0a0d13; }
.alpaca-barbtn label:backdrop { color: #e6e8ee; }   /* same label:backdrop leak as the tabs */

/* --- statuses, labels --- */
.alpaca-status-dot { min-width: 8px; min-height: 8px; border-radius: 4px; background: #5a6375; }
.alpaca-status-dot.ok { background: #22c55e; }
.alpaca-status-dot.warn { background: #f2c94c; }
.alpaca-status-dot.err { background: #ef4444; }
.alpaca-status-spin { min-width: 10px; min-height: 10px; }
.alpaca-mono { font-family: "JetBrains Mono", monospace; font-size: 13px; }

/* --- editor --- */
.alpaca-tab { padding: 4px 10px; }
/* labels are centered by their LINE BOX, but glyph ink hangs ~1.8px low inside it
   (font ascent overhang, measured in pixels) — a 3px bottom pad lifts the line box
   so the ink centers; symmetric padding does nothing (box stays centered) */
.alpaca-tabname { font-size: 13px; font-weight: 500; padding: 0 0 3px 0; }
.alpaca-close { min-width: 18px; min-height: 18px; padding: 0; background: transparent;
                border: none; border-radius: 4px; }
.alpaca-close:hover { background: #1c2230; }
.alpaca-dirty { color: #ef4444; font-size: 8px; }
/* live-file spec (2026-10-05): the amber dot = local edits on a file ALSO changed
   on disk (conflict: next save wins); the deleted-file tab goes italic, dimmed */
.alpaca-dirty.conflict { color: #f2c94c; }
tab .alpaca-tabname.alpaca-deleted, tab:checked .alpaca-tabname.alpaca-deleted {
    font-style: italic; color: #5a6375; }
/* --- side-by-side diff pages (spec §3) --- */
.alpaca-diffsep { min-width: 1px; background: #1c2230; }
.alpaca-diffchipm,.alpaca-diffchipa,.alpaca-diffchipu,.alpaca-diffchipd,.alpaca-diffchipr {
    font-size: 10px; font-weight: 700; }
.alpaca-diffchipm { color: #f2c94c; }
.alpaca-diffchipr { color: #2f80ed; }
.alpaca-diffchipa, .alpaca-diffchipu { color: #22c55e; }
.alpaca-diffchipd { color: #ef4444; }
/* breadcrumb row ~22px + 1px rule. The 20px text inset must be padding, not a
   widget margin — margin sits OUTSIDE the border box, so a margin'd label's
   border-bottom rule starts 20px short of the card's left edge (measured:
   rule from x27, gap at x7..26) */
.alpaca-breadcrumb { color: #8a93a6; font-size: 12px; padding: 2px 12px 2px 20px;
                     border-bottom: 1px solid #1c2230; }

/* --- tab bars: active tab is a raised rounded rect on the darker strip --- */
/* editor header 32 total: no vertical pads — pill fills flush (top border + rule) */
notebook > header { background: #0a0d13; border: none; border-bottom: 1px solid #1c2230;
                    min-height: 32px; padding: 0; }
/* Breeze styles the header via an inset box-shadow (inset 0 -1px #4c4e51) that its own
   border:none does not cover — measured gray hairline rgb(76,78,81) rendered directly
   above our #1c2230 border-bottom on both tab strips. Selector ties .top(+ backdrop)
   specificity so the app provider wins the property. */
notebook > header.top, notebook > header.top:backdrop { box-shadow: none; }
/* Breeze also paints the notebook NODE itself (notebook.frame rule) with
   box-shadow: inset 0 0 0 1px #4c4e51 — border:none doesn't cover it either.
   Measured: a 1px gray vertical leak at the card's inner edges, visible only
   in the breadcrumb row band (the opaque source view below covers it from the
   code row down). Kill the node shadow, not the header's. */
notebook, notebook.frame, notebook:backdrop { box-shadow: none; }
/* no tab borders: a transparent 1px border still costs 2px height (node would
   measure ~34); pill fills the 32px header flush + 1px rule = 33px bar.
   No inter-tab margin: pills touch (user ruling 2026-10-05 — the old 2px side
   margins read as a dead 4px gap); strip edges were flush anyway.
   margin: 0 is LOAD-BEARING: a base rule that sets the property shadows
   Breeze's :checked state margins (`-3px` both sides) at every state; no
   property at all lets them leak — pill box grows on click (nudge) and pulls
   into the neighbours (hover reads as overlap). Probe-measured. */
notebook > header > tabs > tab { min-height: 30px; background: transparent;
                                 border: none; margin: 0; border-radius: 4px 4px 0 0;
                                 padding: 0; outline: none; box-shadow: none; }
notebook > header > tabs > tab:checked { background: #111723;
                                         min-height: 30px; outline: none; box-shadow: none; }
tab > .alpaca-tab { color: #8a93a6; }
tab:checked > .alpaca-tab { color: #e6e8ee; }
tab .alpaca-tabname { color: #8a93a6; }
tab:checked .alpaca-tabname { color: #e6e8ee; }
/* hover = the checked pill's fill, but the label keeps its muted color, so a
   hovered-inactive tab reads dimmer than the checked one. One rule covers BOTH
   notebooks (pane tabs match these selectors too); checked:hover is the same
   fill, so it needs no rule. The app's own transparent base rule is what beats
   Breeze's tab-hover today — this restores a hover under USER priority. */
notebook > header > tabs > tab:hover { background: @alpaca-hover; }

/* --- pane notebook ("Agent"): 28px pill fills the 28px header flush;
     uniform 12px pill insets on the head box (label-side padding left the icon
     flush with the pill edge — measured; 2026-10-05 user ruling: pills get
     WIDER, not gapped — the inset is the text↔pill-side room) --- */
notebook.alpaca-panes > header { padding: 0; min-height: 28px; }
/* margin: 0 shadows Breeze's :checked -3px state margins (see editor rule);
   pills touch here too, matching the editor strip */
notebook.alpaca-panes > header > tabs > tab { min-height: 26px; margin: 0;
                                              border-radius: 4px 4px 0 0; }
notebook.alpaca-panes > header > tabs > tab:checked { min-height: 28px; }
.alpaca-panetab { padding: 0 12px; }
/* ink-centering pad (glyph ink hangs low inside the line box); 2px here (editor
   keeps 3px) drops the pane pill's ink the extra ~1px the user read as centered */
.alpaca-panetabname { font-size: 13px; font-weight: 500; color: #8a93a6; padding: 0 0 2px 0; }
tab:checked .alpaca-panetabname { color: #e6e8ee; }
"""

def _theme_setup() -> None:
    # dark is not system-following in v1 (spec); chrome follows too, not just the editor surface
    Gtk.Settings.get_default().set_property("gtk-application-prefer-dark-theme", True)
    # KDE supplies no decoration layout on Wayland → HeaderBar/WindowControls drew zero
    # window buttons (measured); force the usual right-side triple.
    Gtk.Settings.get_default().set_property("gtk-decoration-layout", ":minimize,maximize,close")
    GtkSource.StyleSchemeManager.get_default().append_search_path(os.path.join(os.path.dirname(__file__), "data"))
    provider = Gtk.CssProvider()
    provider.load_from_string(CSS)
    # USER priority (800) is load-bearing for the button metrics below: at
    # APPLICATION (600) this build lets the theme's button min-height stand
    # (the .alpaca-runpill button min-height rule loses — probe measured 32px
    # button mins under pad-0/min-height-2 at APPLICATION, px-true mins at
    # USER). The app's css IS the user stylesheet v1 ships; nothing else
    # provides USER css, so this changes the cascade, not ownership.
    Gtk.StyleContext.add_provider_for_display(
        Gdk.Display.get_default(), provider, Gtk.STYLE_PROVIDER_PRIORITY_USER)

def run(argv: list[str]) -> None:
    app = Gtk.Application(application_id="io.alpaca.code")

    def on_activate(a):
        _theme_setup()
        path = argv[0] if argv and os.path.isdir(argv[0]) else None
        if not path:
            try:  # state.py lands in Task 2; guarded so the Task-1 skeleton runs without it
                from . import state
                p = state.load().get("last_project")
                if p and os.path.isdir(p):
                    path = p
            except ImportError:
                pass
        w = window.Window(path)
        w.register_actions(app)
        w.win.set_application(a)
        w.win.present()

    app.connect("activate", on_activate)
    app.run([])