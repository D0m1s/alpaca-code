# Responsive check: builds the real Window on the live display (stubbed spawn +
# state, window never presented) and asserts every card can compress below the
# window's ~517px header floor — GTK Paned propagates no child minimums, so a
# widget that refuses to shrink renders oversized and gets clipped instead.
# Needs a display; exit 0 with a skip line when headless.
import os, shutil, sys, tempfile

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import alpaca_code.gi_env as ge
ge.require("Gdk", ("4.0",))
ge.require("Gtk", ("4.0",))

# stub side effects before any layout code runs: no pane children, no state.json
import alpaca_code.state as state
state.save = lambda s: None
state.load = lambda: {"last_project": None, "recents": [], "projects": {}}
import alpaca_code.panels as panels
panels.spawn = lambda *a, **k: None

ge.require("GtkSource", ("5",))
from gi.repository import GLib, Gtk
if not Gtk.init_check():   # gtk_init() fatally exits when the display is unusable
    print("skip: no display")
    sys.exit(0)
from alpaca_code import window as window_mod

root = tempfile.mkdtemp(prefix="alpaca-resp-")
for n in ("package-lock.json", "next.config.ts", "components.json",
          "related-products.tsx", "trude.json"):
    with open(f"{root}/{n}", "w") as f:
        f.write("{}")
deep = f"{root}/deep/path/segments"
os.makedirs(deep)
with open(f"{deep}/segment-file-with-long-name.tsx", "w") as f:
    f.write("x")

w = window_mod.Window(root)     # set_workspace runs: browser + tree populated
for rel in ("package-lock.json", "next.config.ts", "components.json",
            "related-products.tsx", "trude.json",
            "deep/path/segments/segment-file-with-long-name.tsx"):
    w.editor.open_file(f"{root}/{rel}")
GLib.MainContext.default().iteration(may_block=False)

H = Gtk.Orientation.HORIZONTAL
def min_w(widget):
    return widget.measure(H, -1)[0]

def tabname_min():
    """min width of tab0's name label — GTK 4.22 pins scrollable tabs at their
    minimum width, so without a floor on the label the name reads '…'."""
    tl = w.editor.nb.get_tab_label(w.editor.nb.get_nth_page(0))
    c = tl.get_first_child()
    while c:
        if "alpaca-tabname" in c.get_css_classes():
            return min_w(c)
        c = c.get_next_sibling()
    return None

checks = [
    ("window min width (header floor)", w.win, 560),
    ("editor card (6 tabs, deep path)", w.editor, 300),
    ("panes card (3 tabs)", w.panes, 300),
    ("file browser card (long names)", w.tree, 170),
]
fails = 0
for label, widget, limit in checks:
    mw = min_w(widget)
    ok = mw <= limit
    fails += not ok
    print(f"  {'PASS' if ok else 'FAIL'}: {label} min_w={mw} <= {limit}")

tm = tabname_min()
ok = tm is not None and tm >= 60
fails += not ok
print(f"  {'PASS' if ok else 'FAIL'}: tab name text floor min_w={tm} >= 60")

# --- allocation law (mapped window): the distributor gives each tab an equal
# share of the strip and recomputes it as the strip narrows, so tab text must
# (a) never exceed the equal share it would be given, capped at its natural
# width, and (b) be monotonically smaller as the card narrows — GTK 4.22's
# stock scrolled tail does the opposite (widens visible tabs).
import time
from alpaca_code.main import _theme_setup
from alpaca_code.editor import _TabDistributor   # the real CHROME constant, no drift
_theme_setup()   # real fonts/paddings: allocation numbers must match the app
w.win.present()
def pump(sec):
    t0 = time.time(); k = 0
    while time.time() - t0 < sec and k < 800:
        GLib.MainContext.default().iteration(may_block=False); k += 1

def name_widths():
    out = []
    for i in range(w.editor.nb.get_n_pages()):
        tl = w.editor.nb.get_tab_label(w.editor.nb.get_nth_page(i))
        c = tl.get_first_child()
        while c:
            if "alpaca-tabname" in c.get_css_classes():
                out.append(c.get_allocation().width)
                break
            c = c.get_next_sibling()
    return out

pump(0.4)
w.hpane.set_position(900)
pump(0.4)
wide = name_widths()
# equal-share law, captured at this wide position (mirrors _TabDistributor.
# do_allocate exactly): strip = the widget it allocates on (editor._tabs_wrap),
# act_w = END action widget via its None-safe natural measure, CHROME as the
# class constant
act = w.editor.nb.get_action_widget(Gtk.PackType.END)
act_w = act.measure(H, -1)[1] if act else 0
strip_w = w.editor._tabs_wrap.get_allocation().width
share = max((strip_w - act_w) // 6 - _TabDistributor.CHROME, 0)
w.hpane.set_position(850)
pump(0.4)
narrow = name_widths()
# Uniformity is WRONG to demand once the share exceeds a short name's natural
# (cap semantics, CLAUDE.md) — assert instead: counts, per-tab narrow<=wide,
# shrinks overall, and no tab wider than the share the distributor would give.
share_ok = (len(wide) == 6 and len(narrow) == 6
            and all(narrow[i] <= wide[i] for i in range(6))
            and bool(wide) and bool(narrow) and max(narrow) < max(wide)
            and max(wide) <= share)
fails += not share_ok
print(f"  {'PASS' if share_ok else 'FAIL'}: 6 tabs equal-share-capped-at-natural and shrinking with the card "
      f"wide={wide} narrow={narrow} share={share}")
w.win.destroy()

shutil.rmtree(root, ignore_errors=True)
if fails:
    sys.exit(1)
print("responsive: all compress")