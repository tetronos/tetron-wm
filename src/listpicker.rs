//! A filterable, scrolling pick list drawn in the Settings content area
//! (WMAST-002): the ✦ Assistant agent today, themes next. Pure state and
//! render; the owner turns a [`Pick`] into a setting. Render and hit-test
//! share [`Layout`], so a click always lands on the row drawn there.

use crate::buffer::CellBuffer;
use crate::cell::Rgba;

fn pal_bg() -> Rgba { crate::theme::current().window_bg }
fn pal_fg() -> Rgba { crate::theme::current().text }
fn pal_dim() -> Rgba { crate::theme::current().dim }
fn pal_sel() -> Rgba { crate::theme::current().active_bg }
fn pal_accent() -> Rgba { crate::theme::current().accent }

/// One row: what is shown and the key handed back when it is chosen.
#[derive(Clone, Debug, PartialEq)]
pub struct PickItem {
    pub label: String,
    /// Dim text after the label (state, command); also matched by the filter.
    pub detail: String,
    pub key: String,
}

/// What a key or click did.
#[derive(Clone, Debug, PartialEq)]
pub enum Pick {
    None,
    Close,
    Chosen(String),
}

/// Rows of the list inside the content area: title at 1, filter at 3, rows
/// from 5 to two above the bottom (the help line).
struct Layout {
    top: i32,
    rows: usize,
}

impl Layout {
    fn new(h: i32) -> Self {
        Layout { top: 5, rows: (h - 5 - 2).max(1) as usize }
    }
}

pub struct ListPicker {
    title: String,
    items: Vec<PickItem>,
    filter: String,
    /// Index into the filtered rows.
    sel: usize,
    /// First filtered row shown.
    scroll: usize,
    /// Visible rows as of the last render (for keeping the selection in view).
    rows: std::cell::Cell<usize>,
}

impl ListPicker {
    /// A picker with the row whose key is `current` selected.
    pub fn new(title: &str, items: Vec<PickItem>, current: &str) -> Self {
        let sel = items.iter().position(|i| i.key == current).unwrap_or(0);
        let mut p = ListPicker { title: title.into(), items, filter: String::new(), sel, scroll: 0, rows: std::cell::Cell::new(10) };
        p.follow();
        p
    }

    /// Indices of the items the filter keeps (case-insensitive substring of
    /// label or detail).
    fn shown(&self) -> Vec<usize> {
        let f = self.filter.to_lowercase();
        (0..self.items.len())
            .filter(|&i| f.is_empty() || format!("{} {}", self.items[i].label, self.items[i].detail).to_lowercase().contains(&f))
            .collect()
    }

    /// Keep the selection inside the list and on screen.
    fn follow(&mut self) {
        let n = self.shown().len();
        self.sel = self.sel.min(n.saturating_sub(1));
        if self.sel < self.scroll {
            self.scroll = self.sel;
        } else if self.sel >= self.scroll + self.rows.get() {
            self.scroll = self.sel + 1 - self.rows.get();
        }
    }

    /// First row shown in a list of `rows` lines: the kept scroll, moved just
    /// enough to show the selection. Render and click both use it.
    fn view_scroll(&self, rows: usize) -> usize {
        if self.sel < self.scroll {
            self.sel
        } else if self.sel >= self.scroll + rows {
            self.sel + 1 - rows
        } else {
            self.scroll
        }
    }

    pub fn up(&mut self) {
        self.sel = self.sel.saturating_sub(1);
        self.follow();
    }

    pub fn down(&mut self) {
        self.sel += 1;
        self.follow();
    }

    pub fn type_char(&mut self, c: char) {
        if !c.is_control() {
            self.filter.push(c);
            self.sel = 0;
            self.scroll = 0;
        }
    }

    pub fn backspace(&mut self) {
        self.filter.pop();
        self.follow();
    }

    /// Esc clears a filter first, then closes.
    pub fn esc(&mut self) -> Pick {
        if self.filter.is_empty() {
            return Pick::Close;
        }
        self.filter.clear();
        self.follow();
        Pick::None
    }

    pub fn enter(&self) -> Pick {
        self.shown().get(self.sel).map_or(Pick::None, |&i| Pick::Chosen(self.items[i].key.clone()))
    }

    /// A click at content-local `(x, y)` chooses the row under it.
    pub fn click(&mut self, _x: i32, y: i32, h: i32) -> Pick {
        let l = Layout::new(h);
        let row = y - l.top;
        if row < 0 || row as usize >= l.rows {
            return Pick::None;
        }
        let i = self.view_scroll(l.rows) + row as usize;
        if i >= self.shown().len() {
            return Pick::None;
        }
        self.sel = i;
        self.enter()
    }

    /// The wheel moves the selection.
    pub fn scroll_by(&mut self, d: i32) {
        if d < 0 { self.up() } else { self.down() }
    }

    pub fn render(&self, buf: &mut CellBuffer, cx: i32, w: i32, h: i32) {
        let l = Layout::new(h);
        self.rows.set(l.rows);
        let scroll = self.view_scroll(l.rows);
        let width = (w - cx - 1).max(10) as usize;
        let put = |buf: &mut CellBuffer, y: i32, s: &str, fg: Rgba, bg: Rgba| {
            let mut s: String = s.chars().take(width).collect();
            let pad = width.saturating_sub(s.chars().count());
            s.extend(std::iter::repeat_n(' ', pad));
            buf.write_str(cx, y, &s, fg, bg);
        };
        put(buf, 1, &self.title, pal_accent(), pal_bg());
        let shown = self.shown();
        let filter = if self.filter.is_empty() { "type to filter".to_string() } else { format!("{}\u{2588}", self.filter) };
        put(buf, 3, &format!("Filter: {filter}  ({} of {})", shown.len(), self.items.len()), pal_dim(), pal_bg());
        for r in 0..l.rows {
            let y = l.top + r as i32;
            let Some(&i) = shown.get(scroll + r) else {
                put(buf, y, "", pal_fg(), pal_bg());
                continue;
            };
            let it = &self.items[i];
            let sel = scroll + r == self.sel;
            let mark = if sel { "\u{25B8}" } else { " " };
            let (fg, bg) = if sel { (pal_fg(), pal_sel()) } else { (pal_fg(), pal_bg()) };
            put(buf, y, &format!("{mark} {:<20} {}", it.label, it.detail), fg, bg);
        }
        let more = if scroll + l.rows < shown.len() { "\u{25BE} more  " } else { "" };
        put(buf, h - 1, &format!("{more}Up/Down move, Enter chooses, Esc closes"), pal_dim(), pal_bg());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(n: usize) -> Vec<PickItem> {
        (0..n).map(|i| PickItem { label: format!("item{i}"), detail: if i == 3 { "special".into() } else { String::new() }, key: format!("k{i}") }).collect()
    }

    #[test]
    fn opens_on_the_current_key_and_enter_returns_it() {
        let p = ListPicker::new("T", items(5), "k2");
        assert_eq!(p.enter(), Pick::Chosen("k2".into()));
    }

    #[test]
    fn filter_matches_label_or_detail_and_esc_clears_then_closes() {
        let mut p = ListPicker::new("T", items(5), "k0");
        for c in "SPEC".chars() { p.type_char(c); }
        assert_eq!(p.enter(), Pick::Chosen("k3".into()));
        assert_eq!(p.esc(), Pick::None, "first Esc clears the filter");
        assert_eq!(p.esc(), Pick::Close);
    }

    #[test]
    fn scrolls_and_click_hits_the_drawn_row() {
        let mut p = ListPicker::new("T", items(40), "k0");
        let mut buf = CellBuffer::new(60, 12);
        p.render(&mut buf, 0, 60, 12);
        for _ in 0..20 { p.down(); }
        p.render(&mut buf, 0, 60, 12);
        assert_eq!(p.enter(), Pick::Chosen("k20".into()));
        // Layout: rows start at y 5; row 0 is the first shown item.
        let first = p.view_scroll(12 - 7);
        assert_eq!(p.click(3, 5, 12), Pick::Chosen(format!("k{first}")));
        assert_eq!(p.click(3, 2, 12), Pick::None, "the filter line is not a row");
    }
}
