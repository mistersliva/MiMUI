//! A small immediate-mode layout engine: flexbox row/column with wrapping,
//! percentage and intrinsic sizing, absolute children and margins.
//!
//! The algorithm is the classic two-pass flexbox:
//!
//! 1. resolve each child's main-axis base size (flex-basis, explicit size, or content)
//! 2. wrap into lines, then grow/shrink each line independently
//! 3. justify along the main axis, align on the cross axis

use crate::geom::{Dim, Edges, Rect, Vec2};
use crate::style::Align;
use crate::style::{Direction, Justify, Position, Style, WrapMode};

/// One item being laid out by a flex container.
#[derive(Clone, Copy, Debug)]
pub struct Item<'a> {
    pub style: &'a Style,
    /// Size the node's own content wants (text, image).
    pub content: Vec2,
    /// Final position, written by [`flex`].
    pub out_rect: Rect,
}

impl<'a> Item<'a> {
    pub fn new(style: &'a Style) -> Self {
        Self { style, content: Vec2::ZERO, out_rect: Rect::ZERO }
    }

    pub fn with_content(style: &'a Style, content: Vec2) -> Self {
        Self { style, content, out_rect: Rect::ZERO }
    }
}

/// Lays out `items` inside `container`, writing each item's rect.
pub fn flex(container: &Rect, style: &Style, items: &mut [Item<'_>]) {
    let inner = container.inset_edges(resolve_edges(&style.padding, container.size()));
    let row = style.direction == Direction::Row;
    let main_size = if row { inner.w } else { inner.h };
    let cross_size = if row { inner.h } else { inner.w };
    let gap = style.gap.resolve_or(main_size, 0.0).max(0.0);

    let mut flow: Vec<usize> = Vec::with_capacity(items.len());
    let mut absolutes: Vec<usize> = Vec::with_capacity(items.len());
    for (i, it) in items.iter().enumerate() {
        if it.style.position == Position::Absolute {
            absolutes.push(i);
        } else {
            flow.push(i);
        }
    }

    // --- main-axis base sizes ---------------------------------------------
    let mut bases: Vec<f32> = Vec::with_capacity(flow.len());
    for &i in &flow {
        bases.push(base_main(&items[i], row, main_size));
    }

    // --- wrap into lines ---------------------------------------------------
    let lines = wrap_lines(&bases, main_size, gap, style.wrap == WrapMode::Wrap);

    // A single-line container always has a line as tall (or wide) as itself,
    // so `align-items: center` centres within the whole content box.
    let single_line = lines.len() <= 1;

    // --- position each line -----------------------------------------------
    let mut cross_cursor = 0.0f32;

    for line in &lines {
        let sizes = resolve_line_sizes(&line, &bases, &flow, items, main_size, gap);

        let (start, between) = justify_offset(style.justify_content, main_size, &sizes, gap);

        // Cross-axis size of this line: the tallest (or widest) child.
        let line_cross = if single_line {
            cross_size
        } else {
            line.iter()
                .map(|&li| cross_extent(items, flow[li], row, cross_size))
                .fold(0.0f32, f32::max)
        };

        // The first child may be centred on the cross axis, so compute the
        // offset once from the widest child rather than per item.
        let align = style.align_items;
        let mut main_cursor = start;
        for (k, &li) in line.iter().enumerate() {
            let i = flow[li];
            let item_cross = cross_extent(items, i, row, cross_size);
            let cross = clamp_cross(items[i].style, row, cross_size, item_cross);

            let explicit = !if row {
                items[i].style.height
            } else {
                items[i].style.width
            }
            .is_auto();
            let box_cross = if explicit { cross } else { line_cross };
            let cross_pos = match items[i].style.align_self.unwrap_or(align) {
                Align::Center => cross_cursor + (line_cross - box_cross) * 0.5,
                Align::End => cross_cursor + (line_cross - box_cross),
                _ => cross_cursor,
            };

            let main_len = sizes[k] - margin_main(items[i].style, row, main_size);
            let lead = margin_lead(items[i].style, row, main_size);
            let at = main_cursor + lead;

            items[i].out_rect = if row {
                Rect::new(inner.x + at, inner.y + cross_pos, main_len.max(0.0), box_cross)
            } else {
                Rect::new(inner.x + cross_pos, inner.y + at, box_cross, main_len.max(0.0))
            };

            main_cursor += sizes[k] + between;
        }

        cross_cursor += line_cross + gap;
    }

    // --- absolutely positioned children -----------------------------------
    for i in absolutes {
        items[i].out_rect = absolute_rect(&items[i], &inner, row);
    }
}

/// Resolves one dimension, with a guard against percentages that cannot work.
///
/// A container sized by its content has no width to divide by, so `width: 100%`
/// inside it would collapse to nothing. When that happens the box falls back to
/// its content size, which is what the author meant.
pub fn resolve_dim(d: Dim, avail: f32, content: f32) -> f32 {
    match d {
        Dim::Auto => content,
        Dim::Px(px) => px,
        Dim::Pct(p) if p > 0.0 => {
            let v = avail * p / 100.0;
            if v <= 0.0 && content > 0.0 { content } else { v }
        }
        Dim::Pct(_) => 0.0,
    }
}

/// Resolves the child's main-axis base size before growing/shrinking.
///
/// Includes the child's own main-axis margins, since margins sit outside the
/// border box and must consume space.
fn base_main(item: &Item<'_>, row: bool, main_size: f32) -> f32 {
    let s = item.style;
    let basis = s.flex_basis.or(if s.flex_grow > 0.0 { Some(Dim::Auto) } else { None });
    let dim = match basis {
        Some(d) => d,
        // No basis: use the explicit size on the main axis, else content.
        None => if row { s.width } else { s.height },
    };
    let content = if row { item.content.x } else { item.content.y };
    let base = resolve_dim(dim, main_size, content);

    let (min, max) =
        if row { (s.min_width, s.max_width) } else { (s.min_height, s.max_height) };
    let mut v = base.max(0.0);
    if let Some(m) = min {
        v = v.max(m.resolve_or(main_size, 0.0));
    }
    if let Some(m) = max {
        v = v.min(m.resolve_or(main_size, v));
    }
    v + margin_main(s, row, main_size)
}

/// The child's margin on the main axis (leading + trailing).
fn margin_main(s: &Style, row: bool, parent: f32) -> f32 {
    let m = crate::layout::resolve_edges(&s.margin, Vec2::splat(parent));
    if row {
        m.left + m.right
    } else {
        m.top + m.bottom
    }
}

/// The child's leading margin on the main axis.
fn margin_lead(s: &Style, row: bool, parent: f32) -> f32 {
    let m = resolve_edges(&s.margin, Vec2::splat(parent));
    if row { m.left } else { m.top }
}

/// Greedy line breaking.
fn wrap_lines(bases: &[f32], main_size: f32, gap: f32, wrap: bool) -> Vec<Vec<usize>> {
    let mut lines: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut used = 0.0f32;

    for (i, &b) in bases.iter().enumerate() {
        let need = if cur.is_empty() { b } else { b + gap };
        if wrap && !cur.is_empty() && used + need > main_size + 0.001 {
            lines.push(std::mem::take(&mut cur));
            used = 0.0;
        }
        cur.push(i);
        used += if cur.len() > 1 { b + gap } else { b };
    }
    if !cur.is_empty() || lines.is_empty() {
        lines.push(cur);
    }
    lines
}

/// Grows then shrinks a line so it fits (or fills) `main_size`.
fn resolve_line_sizes(
    line: &[usize],
    bases: &[f32],
    flow: &[usize],
    items: &[Item<'_>],
    main_size: f32,
    gap: f32,
) -> Vec<f32> {
    let n = line.len();
    let mut sizes: Vec<f32> = line.iter().map(|&li| bases[li]).collect();
    if n == 0 {
        return sizes;
    }
    let total_gap = gap * (n - 1) as f32;
    let used: f32 = sizes.iter().sum::<f32>() + total_gap;

    // Grow into the leftover space.
    let free = main_size - used;
    if free > 0.0 {
        let grows: Vec<f32> = line.iter().map(|&li| items[flow[li]].style.flex_grow).collect();
        let total: f32 = grows.iter().sum();
        if total > 0.0 {
            for (k, g) in grows.iter().enumerate() {
                sizes[k] += free * (g / total);
            }
        }
    }

    // Shrink on overflow.
    let used: f32 = sizes.iter().sum::<f32>() + total_gap;
    if used > main_size {
        let shrinks: Vec<f32> = line.iter().map(|&li| items[flow[li]].style.flex_shrink).collect();
        let total: f32 = shrinks.iter().sum();
        if total > 0.0 {
            let over = used - main_size;
            for (k, sh) in shrinks.iter().enumerate() {
                // Only items that can shrink contribute.
                let share = sh / total;
                sizes[k] = (sizes[k] - over * share).max(0.0);
            }
        }
    }

    sizes
}

/// The child's extent on the cross axis before alignment.
fn cross_extent(items: &[Item<'_>], i: usize, row: bool, cross_size: f32) -> f32 {
    let s = items[i].style;
    let dim = if row { s.height } else { s.width };
    let content = if row { items[i].content.y } else { items[i].content.x };
    match dim {
        Dim::Auto => content,
        d => resolve_dim(d, cross_size, content),
    }
}

/// Clamps a cross-axis extent to the child's own min/max.
fn clamp_cross(s: &Style, row: bool, cross_size: f32, value: f32) -> f32 {
    let (min, max) =
        if row { (s.min_height, s.max_height) } else { (s.min_width, s.max_width) };
    let mut v = value.max(0.0);
    if let Some(m) = min {
        v = v.max(m.resolve_or(cross_size, 0.0));
    }
    if let Some(m) = max {
        v = v.min(m.resolve_or(cross_size, v));
    }
    v
}

/// Start offset and inter-item spacing for a line.
fn justify_offset(j: Justify, main_size: f32, sizes: &[f32], gap: f32) -> (f32, f32) {
    let n = sizes.len();
    if n == 0 {
        return (0.0, gap);
    }
    let used: f32 = sizes.iter().sum::<f32>() + gap * (n - 1) as f32;
    let free = (main_size - used).max(0.0);

    match j {
        Justify::Start => (0.0, gap),
        Justify::Center => (free / 2.0, gap),
        Justify::End => (free, gap),
        Justify::SpaceBetween => {
            if n == 1 { (0.0, gap) } else { (0.0, free / (n - 1) as f32) }
        }
        Justify::SpaceAround => {
            let each = free / n as f32;
            (each / 2.0, gap + each)
        }
        Justify::SpaceEvenly => {
            let each = free / (n + 1) as f32;
            (each, gap + each)
        }
    }
}

fn absolute_rect(item: &Item<'_>, inner: &Rect, row: bool) -> Rect {
    let s = item.style;
    let e = s.inset;
    let m = resolve_edges(&s.margin, inner.size());

    let (i_x, i_y) = (inner.w, inner.h);
    let content_w = if row { item.content.x } else { item.content.y };
    let content_h = if row { item.content.y } else { item.content.x };

    // Resolve the size first: anchoring from the right or bottom needs to know
    // how big the box is.
    let stretch_x = matches!((s.width, e.left, e.right), (Dim::Auto, d1, d2) if d1.is_definite() && d2.is_definite());
    let stretch_y = matches!((s.height, e.top, e.bottom), (Dim::Auto, d1, d2) if d1.is_definite() && d2.is_definite());

    let w = if stretch_x {
        inner.w - e.left.resolve_or(i_x, 0.0) - e.right.resolve_or(i_x, 0.0)
    } else {
        s.width.resolve_or(i_x, content_w)
    };
    let h = if stretch_y {
        inner.h - e.top.resolve_or(i_y, 0.0) - e.bottom.resolve_or(i_y, 0.0)
    } else {
        s.height.resolve_or(i_y, content_h)
    };

    let x = if e.left.is_definite() {
        inner.x + e.left.resolve_or(i_x, 0.0)
    } else if e.right.is_definite() {
        inner.max().x - e.right.resolve_or(i_x, 0.0) - w
    } else {
        inner.x
    };
    let y = if e.top.is_definite() {
        inner.y + e.top.resolve_or(i_y, 0.0)
    } else if e.bottom.is_definite() {
        inner.max().y - e.bottom.resolve_or(i_y, 0.0) - h
    } else {
        inner.y
    };

    Rect { x: x + m.left, y: y + m.top, w, h }
}

/// Resolves `padding`/`inset` against the parent size.
pub fn resolve_edges(e: &Edges<Dim>, parent: Vec2) -> Edges<f32> {
    Edges {
        top: e.top.resolve_or(parent.y, 0.0),
        right: e.right.resolve_or(parent.x, 0.0),
        bottom: e.bottom.resolve_or(parent.y, 0.0),
        left: e.left.resolve_or(parent.x, 0.0),
    }
}

/// The size a node wants, given its own content and its children's sizes.
///
/// Returns the **border-box** size: padding and border are included, margins are
/// not. Children are expected to have been measured without their own margins,
/// so each child's margin is added here.
pub fn measure(style: &Style, own: Vec2, children: &[Vec2], parent: Vec2) -> Vec2 {
    let pad = resolve_edges(&style.padding, parent);
    let row = style.direction == Direction::Row;
    let gap = style.gap.resolve_or(if row { parent.x } else { parent.y }, 0.0);

    // Content of this node's own body (text or image).
    let (own_main, own_cross) = if row { (own.x, own.y) } else { (own.y, own.x) };

    if children.is_empty() {
        let (w, h) = apply_size(style, own.x, own.y, parent);
        return Vec2::new(w + pad.horizontal(), h + pad.vertical());
    }

    let mut main = own_main;
    let mut cross = own_cross;
    for (i, c) in children.iter().enumerate() {
        let (cm, cc) = if row { (c.x, c.y) } else { (c.y, c.x) };
        main += cm;
        cross = cross.max(cc);
        if i + 1 < children.len() {
            main += gap;
        }
    }

    let (cw, ch) = if row { (main, cross) } else { (cross, main) };
    let (w, h) = apply_size(style, cw, ch, parent);
    Vec2::new(w + pad.horizontal(), h + pad.vertical())
}

/// Like [`measure`] but also counts each child's margin, which layout needs when
/// packing siblings.
pub fn measure_with_margins(style: &Style, children: &[(Vec2, Vec2)], parent: Vec2) -> Vec2 {
    let mut sizes = Vec::with_capacity(children.len());
    for (size, margin) in children {
        sizes.push(Vec2::new(size.x + margin.x, size.y + margin.y));
    }
    let _ = style;
    measure(style, Vec2::ZERO, &sizes, parent)
}

/// The resolved margins of a style against a parent size.
pub fn margins(style: &Style, parent: Vec2) -> Vec2 {
    let m = resolve_edges(&style.margin, parent);
    Vec2::new(m.horizontal(), m.vertical())
}

/// Applies width/height/min/max to a content size.
pub fn apply_size(style: &Style, content_w: f32, content_h: f32, parent: Vec2) -> (f32, f32) {
    let mut w = resolve_dim(style.width, parent.x, content_w);
    let mut h = resolve_dim(style.height, parent.y, content_h);
    if let Some(m) = style.min_width {
        w = w.max(m.resolve_or(parent.x, 0.0));
    }
    if let Some(m) = style.max_width {
        w = w.min(m.resolve_or(parent.x, w));
    }
    if let Some(m) = style.min_height {
        h = h.max(m.resolve_or(parent.y, 0.0));
    }
    if let Some(m) = style.max_height {
        h = h.min(m.resolve_or(parent.y, h));
    }
    (w.max(0.0), h.max(0.0))
}

/// True when this style clips its children's paint.
pub fn clips(style: &Style) -> bool {
    style.clip || style.overflow != crate::style::Overflow::Visible
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(s: &str) -> Style {
        Style::parse(s)
    }

    fn lay(container: Rect, s: &Style, styles: &[&Style]) -> Vec<Rect> {
        let mut items: Vec<Item<'_>> = styles.iter().map(|s| Item::new(s)).collect();
        flex(&container, s, &mut items);
        items.iter().map(|i| i.out_rect).collect()
    }

    #[test]
    fn row_places_children_left_to_right() {
        let s = st("direction: row; gap: 10px;");
        let a = st("width: 50px; height: 20px;");
        let b = st("width: 30px; height: 20px;");
        let r = lay(Rect::new(0.0, 0.0, 200.0, 100.0), &s, &[&a, &b]);
        assert_eq!(r[0], Rect::new(0.0, 0.0, 50.0, 20.0));
        assert_eq!(r[1].x, 60.0);
        assert_eq!(r[1].y, 0.0);
    }

    #[test]
    fn column_stacks_top_to_bottom() {
        let s = st("direction: column; gap: 4px;");
        let a = st("width: 20px; height: 10px;");
        let b = st("width: 20px; height: 10px;");
        let r = lay(Rect::new(0.0, 0.0, 100.0, 100.0), &s, &[&a, &b]);
        assert_eq!(r[0].y, 0.0);
        assert_eq!(r[1].y, 14.0);
    }

    #[test]
    fn grow_splits_the_leftover_space() {
        let s = st("direction: row;");
        let a = st("flex-grow: 1; height: 10px;");
        let b = st("flex-grow: 1; height: 10px;");
        let r = lay(Rect::new(0.0, 0.0, 100.0, 50.0), &s, &[&a, &b]);
        assert!((r[0].w - 50.0).abs() < 0.01, "{}", r[0].w);
        assert!((r[1].w - 50.0).abs() < 0.01, "{}", r[1].w);
    }

    #[test]
    fn padding_insets_children() {
        let s = st("direction: row; padding: 10px;");
        let a = st("width: 20px; height: 20px;");
        let r = lay(Rect::new(0.0, 0.0, 200.0, 200.0), &s, &[&a]);
        assert_eq!(r[0].x, 10.0);
        assert_eq!(r[0].y, 10.0);
    }

    #[test]
    fn justify_center_centers_the_group() {
        let s = st("direction: row; justify-content: center;");
        let a = st("width: 40px; height: 10px;");
        let r = lay(Rect::new(0.0, 0.0, 100.0, 50.0), &s, &[&a]);
        assert_eq!(r[0].x, 30.0);
    }

    #[test]
    fn justify_space_between_pushes_apart() {
        let s = st("direction: row; justify-content: space-between;");
        let a = st("width: 20px; height: 10px;");
        let b = st("width: 20px; height: 10px;");
        let r = lay(Rect::new(0.0, 0.0, 100.0, 50.0), &s, &[&a, &b]);
        assert_eq!(r[0].x, 0.0);
        assert_eq!(r[1].x, 80.0);
    }

    #[test]
    fn percentage_resolves_against_the_parent() {
        let s = st("direction: row;");
        let a = st("width: 50%; height: 10px;");
        let r = lay(Rect::new(0.0, 0.0, 200.0, 100.0), &s, &[&a]);
        assert_eq!(r[0].w, 100.0);
    }

    #[test]
    fn align_center_centres_on_the_cross_axis() {
        let s = st("direction: row; align-items: center; height: 100px;");
        let a = st("width: 20px; height: 20px;");
        let r = lay(Rect::new(0.0, 0.0, 100.0, 100.0), &s, &[&a]);
        assert_eq!(r[0].y, 40.0);
    }

    #[test]
    fn wraps_into_two_lines() {
        let s = st("direction: row; flex-wrap: wrap;");
        let a = st("width: 60px; height: 10px;");
        let b = st("width: 60px; height: 10px;");
        let r = lay(Rect::new(0.0, 0.0, 100.0, 200.0), &s, &[&a, &b]);
        assert_eq!(r[0].y, 0.0);
        assert!(r[1].y >= 10.0, "expected wrap, got y={}", r[1].y);
    }

    #[test]
    fn shrink_prevents_overflow() {
        let s = st("direction: row;");
        let a = st("width: 100px; height: 10px;");
        let b = st("width: 100px; height: 10px;");
        let r = lay(Rect::new(0.0, 0.0, 100.0, 100.0), &s, &[&a, &b]);
        let total = r[0].w + r[1].w;
        assert!(total <= 100.5, "overflowed: {}", total);
    }

    #[test]
    fn absolute_children_ignore_the_flow() {
        let s = st("direction: row;");
        let a = st("position: absolute; inset: 5px; width: 10px; height: 10px;");
        let r = lay(Rect::new(0.0, 0.0, 200.0, 200.0), &s, &[&a]);
        assert_eq!(r[0].x, 5.0);
        assert_eq!(r[0].y, 5.0);
    }

    #[test]
    fn absolute_pins_to_the_bottom_right() {
        let s = st("direction: row;");
        let a = st(
            "position: absolute; right: 10px; bottom: 20px; width: 10px; height: 10px;",
        );
        let r = lay(Rect::new(0.0, 0.0, 100.0, 100.0), &s, &[&a]);
        assert_eq!(r[0].x, 80.0);
        assert_eq!(r[0].y, 70.0);
    }

    #[test]
    fn margins_push_siblings_apart() {
        let s = st("direction: row;");
        let a = st("width: 20px; height: 10px; margin-right: 8px;");
        let b = st("width: 20px; height: 10px;");
        let r = lay(Rect::new(0.0, 0.0, 100.0, 50.0), &s, &[&a, &b]);
        assert_eq!(r[1].x, 28.0);
    }

    #[test]
    fn measure_accounts_for_children_and_padding() {
        let s = st("direction: column; gap: 5px; padding: 4px;");
        // Two 10px-tall rows with a 5px gap: 10 + 5 + 10 = 25, plus 4px padding
        // top and bottom gives 33. The widest row is 30px, plus 8 gives 38.
        let kids = [Vec2::new(20.0, 10.0), Vec2::new(30.0, 10.0)];
        let size = measure(&s, Vec2::ZERO, &kids, Vec2::new(100.0, 100.0));
        assert!((size.x - 38.0).abs() < 0.01, "width {}", size.x);
        assert!((size.y - 33.0).abs() < 0.01, "height {}", size.y);
    }

    #[test]
    fn measure_uses_text_content() {
        let s = st("font-size: 16px;");
        let size = measure(&s, Vec2::new(120.0, 20.0), &[], Vec2::new(500.0, 500.0));
        assert_eq!(size, Vec2::new(120.0, 20.0));
    }

    #[test]
    fn max_width_caps_a_child() {
        let s = st("direction: row;");
        let a = st("width: 500px; max-width: 50px; height: 10px;");
        let r = lay(Rect::new(0.0, 0.0, 500.0, 100.0), &s, &[&a]);
        assert!((r[0].w - 50.0).abs() < 0.01, "got {}", r[0].w);
    }

    #[test]
    fn max_width_caps_a_container_when_measured() {
        // A container's own max-width is applied when its parent measures it,
        // not by `flex` (which is handed an already-sized box).
        let s = st("direction: row; max-width: 50px;");
        let (w, _) = apply_size(&s, 500.0, 10.0, Vec2::new(500.0, 100.0));
        assert!((w - 50.0).abs() < 0.01, "got {}", w);
    }

    #[test]
    fn min_width_floors_a_child() {
        let s = st("direction: row;");
        let a = st("width: 10px; min-width: 80px; height: 10px;");
        let r = lay(Rect::new(0.0, 0.0, 500.0, 100.0), &s, &[&a]);
        assert!((r[0].w - 80.0).abs() < 0.01, "got {}", r[0].w);
    }
}