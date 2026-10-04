import os
import alpaca_code.gi_env as ge
ge.require("Gdk", ("4.0",))
ge.require("Gtk", ("4.0",))
ge.require("GtkSource", ("5",))
from gi.repository import Gdk, Gtk, GtkSource
from . import window

CSS = """
window { background: #07090d; }
.alpaca-surface1 { background: #0a0d13; }
.alpaca-surface2 { background: #0d1017; }
.alpaca-surface3 { background: #141720; }
.alpaca-header button { background: #0a0d13; color: #cbd5e1; border: none; border-radius: 6px; padding: 6px 12px; }
.alpaca-header menubutton > button { background: transparent; color: #cbd5e1; padding: 4px 10px; }
.alpaca-run { color: white; background: #22c55e; }
.alpaca-stop { color: white; background: #ef4444; }
.alpaca-run:disabled, .alpaca-stop:disabled { background: #1a1f29; color: #4b5563; }
.alpaca-status-dot { min-width: 8px; min-height: 8px; border-radius: 4px; background: #6b7282; }
.alpaca-status-dot.ok { background: #22c55e; }
.alpaca-status-dot.err { background: #ef4444; }
.alpaca-muted { color: #6b7282; font-size: 11px; }
.alpaca-mono { font-family: monospace; font-size: 11pt; }
.alpaca-tree { background: transparent; color: #cbd5e1; }
.alpaca-tree .view:selected { background: #1b3560; }
.alpaca-accent { color: #2f80ed; }
.alpaca-search { background: #0a0d13; color: #cbd5e1; }
.alpaca-status-row { color: #6b7282; font-size: 11px; padding: 8px 12px; background: #0a0d13; }
.alpaca-tab { padding: 4px 8px; color: #cbd5e1; }
.alpaca-close { min-width: 18px; min-height: 18px; padding: 0; color: #6b7282; background: transparent; border: none; }
.alpaca-dirty { color: #ef4444; font-size: 9px; }
.alpaca-breadcrumb { color: #6b7282; font-size: 11px; padding: 4px 8px; }
notebook > header { background: #0a0d13; border: none; }
notebook > header > tabs > tab { background: #0a0d13; color: #cbd5e1; padding: 4px 6px; }
notebook > header > tabs > tab:checked { background: #111723; border-bottom: 2px solid #2f80ed; }
"""

def _theme_setup() -> None:
    # dark is not system-following in v1 (spec); chrome follows too, not just the editor surface
    Gtk.Settings.get_default().set_property("gtk-application-prefer-dark-theme", True)
    GtkSource.StyleSchemeManager.get_default().append_search_path(os.path.join(os.path.dirname(__file__), "data"))
    provider = Gtk.CssProvider()
    provider.load_from_string(CSS)
    Gtk.StyleContext.add_provider_for_display(
        Gdk.Display.get_default(), provider, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION)

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