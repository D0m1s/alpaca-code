//! Hover-tree: gtk::TreeView subclass — hover-band painting + scroll-adj sync
//! + press-position capture. Moves here from filetree.rs (S2) so the git
//! changes view (gitview.rs) can reuse it without importing filetree internals.

#![allow(deprecated)]

use std::cell::{Cell, RefCell};

use relm4::gtk;
use relm4::gtk::prelude::*;
use relm4::gtk::subclass::prelude::*;
use relm4::gtk::{glib, graphene};

// Manual Default: BOTH sentinel cells start (-1.0, -1.0) — derive(Default)
// would start them (0.0, 0.0), a legal-looking pointer at widget origin that
// defeats px<0 (press) and y<0 (hover) laws and paints a phantom row-0 band.
pub struct Imp {
    hover_xy: Cell<(f64, f64)>, // last pointer pos in view coords; y<0 = outside
    hover_row: RefCell<Option<gtk::TreeRowReference>>,
    last_adj: RefCell<Option<gtk::Adjustment>>,
    press_xy: Cell<(f64, f64)>, // python's view._press_xy sentinel
}

impl Default for Imp {
    fn default() -> Self {
        Self {
            hover_xy: Cell::new((-1.0, -1.0)),
            hover_row: RefCell::new(None),
            last_adj: RefCell::new(None),
            press_xy: Cell::new((-1.0, -1.0)),
        }
    }
}

#[glib::object_subclass]
impl ObjectSubclass for Imp {
    const NAME: &'static str = "AlpacaHoverTree";
    type Type = HoverTree; // file level: super:: would be the crate root (mod hover nested → super was hover)
    type ParentType = gtk::TreeView;
}

impl ObjectImpl for Imp {
    fn constructed(&self) {
        self.parent_constructed();
        let obj = self.obj();

        let motion = gtk::EventControllerMotion::new();
        let this = obj.clone();
        motion.connect_motion(move |_, x, y| this.imp().on_motion(x, y));
        let this = obj.clone();
        motion.connect_leave(move |_| this.imp().on_leave());
        obj.add_controller(motion);
        // capture-phase press probe: runs before the treeview's own
        // handling (changes view routes hit tests by x in S2)
        let press = gtk::GestureClick::new();
        press.set_propagation_phase(gtk::PropagationPhase::Capture);
        let press2 = obj.clone();
        press.connect_pressed(move |_g, _n, x, y| {
            press2.imp().press_xy.set((x, y));
        });
        obj.add_controller(press);
        // the hook attaches to whatever adjustment is live right now — the
        // tree's own at construct, the ScrolledWindow's once parented
        // (the property swap fires notify::vadjustment → re-hook)
        obj.connect_notify_local(Some("vadjustment"), |o, _| o.imp().hook_scroll_adj());
        obj.imp().hook_scroll_adj();
    }
}

impl TreeViewImpl for Imp {}

impl WidgetImpl for Imp {
    fn snapshot(&self, snapshot: &gtk::Snapshot) {
        if let Some((x, y, w, h)) = self.hover_band() {
            // python: lookup_color("alpaca-hover") else "#111723"
            let rgba = self
                .obj()
                .style_context()
                .lookup_color("alpaca-hover")
                .unwrap_or_else(|| fallback_hover());
            let rect = graphene::Rect::new(x, y, w, h);
            snapshot.append_color(&rgba, &rect);
        }
        self.parent_snapshot(snapshot);
    }
}

fn fallback_hover() -> gtk::gdk::RGBA {
    gtk::gdk::RGBA::new(17.0 / 255.0, 23.0 / 255.0, 35.0 / 255.0, 1.0)
}

impl Imp {
    fn hook_scroll_adj(&self) {
        let obj = self.obj();
        let Some(adj) = obj.vadjustment() else { return };
        if *self.last_adj.borrow() == Some(adj.clone()) {
            return;
        }
        *self.last_adj.borrow_mut() = Some(adj.clone());
        let weak = obj.downgrade();
        adj.connect_value_changed(move |_| {
            // defer past the treeview's own scroll sync (it connects after
            // ours; a synchronous refresh would read pre-scroll geometry)
            let Some(o) = weak.upgrade() else { return };
            gtk::glib::idle_add_local_once(move || o.refresh_hover());
        });
    }

    fn on_motion(&self, x: f64, y: f64) {
        self.hover_xy.set((x, y));
        self.obj().refresh_hover();
    }

    fn on_leave(&self) {
        self.hover_xy.set((-1.0, -1.0));
        self.hover_row.borrow_mut().take();
        self.obj().queue_draw();
    }

    pub fn refresh_hover(&self) {
        let obj = self.obj();
        let (x, y) = self.hover_xy.get();
        if y < 0.0 {
            return;
        }
        // None below the last row; same row → no repaint (scroll/mutations
        // already damage). TreePath::compare is private in this binding —
        // PartialEq::eq wraps it.
        let new = obj.path_at_pos(x as i32, y as i32).and_then(|(p, _, _, _)| p);
        let old = self
            .hover_row
            .borrow()
            .as_ref()
            .filter(|r| r.valid())
            .and_then(|r| r.path());
        if let (Some(a), Some(b)) = (old.as_ref(), new.as_ref()) {
            if a == b {
                return;
            }
        }
        let model = obj.model();
        let row = new.as_ref().and_then(|p| {
            model
                .as_ref()
                .and_then(|m| gtk::TreeRowReference::new(m, p))
        });
        *self.hover_row.borrow_mut() = row;
        obj.queue_draw();
    }

    /// Hovered row's strip in widget coords, or None.
    fn hover_band(&self) -> Option<(f32, f32, f32, f32)> {
        let obj = self.obj();
        let r = self.hover_row.borrow().clone()?;
        if !r.valid() {
            return None;
        }
        let path = r.path()?;
        let col = obj.columns().first()?.clone();
        // background rects arrive in viewport coords already — no
        // get_visible_rect() conversion, that would double-subtract
        let area = obj.background_area(Some(&path), Some(&col));
        if area.height() <= 0 {
            return None;
        }
        // band spans the row's FULL width, not col-0's
        let alloc = obj.allocation();
        Some((
            area.x() as f32,
            area.y() as f32,
            (alloc.width() - area.x()).max(0) as f32,
            area.height() as f32,
        ))
    }
}

glib::wrapper! {
    pub struct HoverTree(ObjectSubclass<Imp>)
        @extends gtk::TreeView, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Scrollable;
}

impl Default for HoverTree {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl HoverTree {
    pub fn refresh_hover(&self) {
        self.imp().refresh_hover();
    }

    /// Pressed position captured at GestureClick press (capture phase — runs
    /// before the treeview's own handling). (-1.0, -1.0) = no press.
    pub fn press_xy(&self) -> (f64, f64) {
        self.imp().press_xy.get()
    }
}

/// Changes-view toggle routing (python gitview.py `_toggle_hit`): press_x
/// inside the toggle CELL AREA, past the chevron slot (+16 = 12px art + 2px
/// pad, measured), within the toggle's natural width (falls back to 24 at
/// the caller when the lazy measure fails). px<0 = no press captured.
pub fn toggle_hit(px: f64, ca_x: i32, tog_w: i32) -> bool {
    if px < 0.0 {
        return false;
    }
    let x0 = ca_x + 16;
    let x1 = (x0 + tog_w) as f64;
    (x0 as f64) <= px && px < x1
}

#[cfg(test)]
mod tests {
    use super::toggle_hit;
    use super::Imp;

    #[test]
    fn default_sentinels() {
        // T3 ruling: derive(Default) started both cells at (0.0, 0.0) — a
        // legal-looking pointer position that defeats the press sentinel law
        // (px<0 = "no press") and reads as "pointer at 0,0" for hover.
        let imp = Imp::default();
        assert_eq!(imp.hover_xy.get(), (-1.0, -1.0));
        assert_eq!(imp.press_xy.get(), (-1.0, -1.0));
    }

    #[test]
    fn toggle_hit_geometry() {
        // python gitview.py _toggle_hit: px<0 = no press → never a toggle;
        // x0 = cell_area.x + 16 (chevron slot: 12px art + 2px pad, measured);
        // inclusive lower, exclusive upper.
        assert!(!toggle_hit(-5.0, 0, 24), "sentinel press: not a toggle hit");
        assert!(toggle_hit(20.0, 4, 24), "x0 = 20: lower bound inclusive (20..44)");
        assert!(!toggle_hit(19.0, 4, 24), "below x0: still the chevron slot, not the toggle");
        assert!(!toggle_hit(44.0, 4, 24), "upper bound exclusive");
        assert!(toggle_hit(43.9, 4, 24), "just inside is a hit");
        assert!(!toggle_hit(10.0, 4, 24), "chevron-side click is the row, not the toggle");
    }
}