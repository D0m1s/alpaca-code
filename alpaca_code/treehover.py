# Hover-band TreeView — shared by the file browser and the changes view.
#
# GTK4 tree rows expose no :hover state: gtktreeview folds the widget's own
# state into EVERY row's background paint (and never sets cell PRELIT —
# gtkcellrenderer.c:1653 strips it), so a `treeview:hover` CSS rule would tint
# the whole widget. Instead: pointer position tracked via a motion controller,
# kept as a TreeRowReference (stable across model shifts above the row;
# `.valid()` goes False when the row is deleted), painted in do_snapshot
# BEFORE the chain-up — rows' transparent CSS background lets the band show,
# while opaque paint (selected #1b3560, cells' text) draws on top of it.
import alpaca_code.gi_env as ge
ge.require("Gtk", ("4.0",))
ge.require("Gdk", ("4.0",))
ge.require("Pango", ("1.0",))
ge.require("Graphene", ("1.0",))
from gi.repository import Gdk, GLib, Gtk, Graphene


class HoverTree(Gtk.TreeView):
    def __init__(self, **kw):
        super().__init__(**kw)
        self._hover_xy = (-1.0, -1.0)   # last pointer pos in view coords; y<0 = outside
        self._hover_row = None          # Gtk.TreeRowReference | None
        motion = Gtk.EventControllerMotion()
        motion.connect("motion", self._on_motion)
        motion.connect("leave", self._on_leave)
        self.add_controller(motion)
        # defer past the treeview's own scroll sync (it connects after this one,
        # so a synchronous refresh here would read pre-scroll geometry). The
        # hook attaches to whatever adjustment is live at the moment — the tree's
        # own at construct time, the ScrolledWindow's once it's parented (the
        # property swap fires notify::vadjustment and we reconnect).
        self._scroll_adj = None
        self.connect("notify::vadjustment", self._hook_scroll_adj)
        self._hook_scroll_adj()

    def _hook_scroll_adj(self, *a):
        adj = self.get_vadjustment()
        if adj is self._scroll_adj:
            return
        self._scroll_adj = adj
        adj.connect("value-changed",
                    lambda a: GLib.idle_add(self._refresh_hover))

    def _on_motion(self, ctrl, x, y):
        self._hover_xy = (x, y)
        self._refresh_hover()

    def _on_leave(self, ctrl):
        self._hover_xy = (-1.0, -1.0)
        self._hover_row = None
        self.queue_draw()

    def _refresh_hover(self):
        x, y = self._hover_xy
        if y < 0:
            return
        res = self.get_path_at_pos(int(x), int(y))   # None below the last row
        old = self._hover_row.get_path() if self._hover_row and self._hover_row.valid() else None
        new = res[0] if res else None
        if old is not None and new is not None and old.compare(new) == 0:
            return     # same row: no repaint (scroll/mutations already damage)
        self._hover_row = Gtk.TreeRowReference.new(self.get_model(), new) if new else None
        self.queue_draw()

    def _hover_area(self):
        """Graphene.Rect of the hovered row's strip in widget coords, or None."""
        if not self._hover_row or not self._hover_row.valid():
            return None
        path = self._hover_row.get_path()
        cols = self.get_columns()
        if not path or not cols:
            return None
        # measured + gtktreeview.c (get_row_y_offset subtracts the scroll offset
        # itself): background rects arrive in VISIBLE/viewport coords already —
        # no get_visible_rect() conversion, that would double-subtract
        area = self.get_background_area(path, cols[0])
        if area is None or area.height <= 0:
            return None
        # band spans the row's FULL width, not col-0's: the file tree's band
        # covers its (single wide) column's badges, and the changes view's
        # col-0 rect is the column NATURAL (54px measured) while the painted
        # column stretches — a col-width band is a sliver under just the name.
        rect = Graphene.Rect()
        rect.init(area.x, area.y, max(self.get_allocation().width - area.x, 0.0), area.height)
        return rect

    def do_snapshot(self, snapshot):
        area = self._hover_area()
        if area is not None:
            ok, rgba = self.get_style_context().lookup_color("alpaca-hover")
            if not ok:
                rgba = Gdk.RGBA(); rgba.parse("#111723")
            snapshot.append_color(rgba, area)
        Gtk.TreeView.do_snapshot(self, snapshot)