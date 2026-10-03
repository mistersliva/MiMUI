//! The widget layer: everything you can write in a `ui!` block.
//!
//! Each widget resolves its attributes into styles, then contributes one or
//! more nodes to the tree. Tags are matched case-insensitively.

use crate::anim::{AnimSpec, Animation};
use crate::color::{parse_color, Color};
use crate::css;
use crate::geom::{Corners, Dim, Edges, Vec2};
use crate::image::{Fit, ImageKind};
use crate::input::Key;
use crate::style::{
    Align, BoxShadow, CursorKind, Direction, Justify, Overflow, Position, Style, State,
    TriggerStyles,
};
use crate::text::TextAttrs;
use crate::UiCtx;

/// A value from the `ui!` macro.
#[derive(Clone, Debug, PartialEq)]
pub enum Attr {
    Str(String),
    Num(f32),
    Bool(bool),
    Ident(String),
    /// The result of interpolating a Rust expression, as its `Display` form.
    Text(String),
}

impl Attr {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Attr::Str(s) | Attr::Text(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_num(&self) -> Option<f32> {
        match self {
            Attr::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Attr::Bool(b) => Some(*b),
            Attr::Ident(s) if s == "true" => Some(true),
            Attr::Ident(s) if s == "false" => Some(false),
            _ => None,
        }
    }

    /// A loose string form, so `link=123` and `link="123"` behave alike.
    pub fn to_text(&self) -> String {
        match self {
            Attr::Str(s) | Attr::Text(s) => s.clone(),
            Attr::Num(n) => {
                if n.fract() == 0.0 { format!("{}", *n as i64) } else { n.to_string() }
            }
            Attr::Bool(b) => b.to_string(),
            Attr::Ident(s) => s.clone(),
        }
    }
}

/// What a widget declared, after the cascade ran.
pub struct WidgetSpec<'a> {
    pub tag: &'a str,
    pub attrs: &'a [(&'static str, Attr)],
    /// Text from the first positional argument.
    pub text: Option<&'a str>,
}

impl WidgetSpec<'_> {
    pub fn get(&self, key: &str) -> Option<&Attr> {
        self.attrs.iter().find(|(k, _)| *k == key).map(|(_, v)| v)
    }

    /// A string attribute, falling back to a loose text form.
    pub fn text_of(&self, key: &str) -> Option<String> {
        self.get(key).map(|a| a.to_text())
    }

    pub fn str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(|a| a.as_str())
    }

    /// A string value, falling back to a positional text argument.
    pub fn label(&self) -> String {
        self.any_str(&["text", "label", "title", "value"])
            .map(str::to_string)
            .or_else(|| self.text.map(str::to_string))
            .unwrap_or_default()
    }

    pub fn num(&self, key: &str) -> Option<f32> {
        self.get(key).and_then(|a| a.as_num())
    }

    pub fn num_or(&self, key: &str, default: f32) -> f32 {
        self.num(key).unwrap_or(default)
    }

    pub fn flag(&self, key: &str) -> bool {
        self.get(key).and_then(|a| a.as_bool()).unwrap_or(false)
    }

    /// The first present value among `keys`, as a string.
    pub fn any_str(&self, keys: &[&str]) -> Option<&str> {
        keys.iter().find_map(|k| self.str(k))
    }

    pub fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }
}

/// What a node does with clicks and keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionKind {
    Button,
    Checkbox,
    Radio,
    Slider,
    TextInput,
    /// Inert: hover styling only.
    None,
}

/// The result of the attribute cascade for one element.
pub struct Resolved {
    pub styles: TriggerStyles,
    pub animations: Vec<AnimSpec>,
    pub id: Option<String>,
    pub disabled: bool,
}

impl Clone for Resolved {
    fn clone(&self) -> Self {
        Self {
            styles: self.styles.clone(),
            animations: self.animations.clone(),
            id: self.id.clone(),
            disabled: self.disabled,
        }
    }
}

impl Resolved {
    /// Runs the cascade: classes, inline style, per-state blocks, animations.
    /// An empty cascade result, for widgets that build styles by hand.
    pub fn blank() -> Self {
        Resolved {
            styles: TriggerStyles::default(),
            animations: Vec::new(),
            id: None,
            disabled: false,
        }
    }

    /// A cascade result whose base style is `style`.
    pub fn of(style: Style) -> Self {
        Resolved {
            styles: TriggerStyles { base: style, ..Default::default() },
            animations: Vec::new(),
            id: None,
            disabled: false,
        }
    }

    pub fn from_spec(cx: &mut UiCtx<'_>, spec: &WidgetSpec<'_>) -> Resolved {
        // Start from the identity so only declared values survive.
        let mut out = Resolved::blank();

        // `class="a b c"` may also arrive as several `class=` attributes.
        let mut classes: Vec<String> = Vec::new();
        for (k, v) in spec.attrs {
            match *k {
                "class" | "classes" => {
                    classes.extend(v.to_text().split_whitespace().map(str::to_string));
                }
                "id" => out.id = Some(v.to_text()),
                _ => {}
            }
        }

        // 1. class rules, copied out so `cx` stays free for later frames.
        //    Later classes win over earlier ones, as in CSS.
        for c in &classes {
            if let Some(decls) = cx.css_for(c) {
                let decls = decls.to_string();
                css::apply_block(&mut out.styles.base, &decls);
            }
        }

        // 2. properties written inline as attributes: `bg="#fff" w="50px"`.
        //    This is what makes `Button "ok" radius="4px"` work. State blocks and
        //    `style=` are handled below, so they must not land here.
        for (k, v) in spec.attrs {
            if matches!(*k, "style" | "css" | "class" | "classes" | "id")
                || Style::state_of(k).is_some()
            {
                continue;
            }
            if crate::css::is_property(k) {
                let decl = format!("{k}: {}", v.to_text());
                css::apply_block(&mut out.styles.base, &decl);
            }
        }

        // 3. an explicit `style:` block wins over both.
        for (k, v) in spec.attrs {
            if *k == "style" || *k == "css" {
                let mut s = Style::default();
                css::apply_block(&mut s, &v.to_text());
                out.styles.base = crate::style::merge_partial(&out.styles.base, &s);
            }
        }

        // 4. per-state blocks, each inheriting the base.
        let state_blocks: Vec<(State, String)> = spec
            .attrs
            .iter()
            .filter_map(|(k, v)| Style::state_of(k).map(|s| (s, v.to_text())))
            .collect();
        for (state, block) in state_blocks {
            let mut partial = Style::default();
            css::apply_block(&mut partial, &block);
            let base = out.styles.base.clone();
            out.styles.set_state(state, &partial, &base);
        }

        // 5. animations.
        for (k, v) in spec.attrs {
            if *k == "animation" || *k == "anim" {
                if let Some(a) = crate::anim::parse_anim_spec(&v.to_text()) {
                    out.animations.push(a);
                }
            }
        }

        out.disabled = out.styles.disabled.is_some() || spec.flag("disabled");
        out
    }

    /// Fills in widget defaults wherever the user said nothing.
    ///
    /// This is what keeps `Button "x" padding="4px"` working while giving a
    /// bare `Button "x"` a sensible look.
    pub fn merge_defaults(&mut self, defaults: &Style) {
        let d = Style::default();

        macro_rules! fields {
            ($m:ident, $target:expr, $from:expr) => {
                $m!($target, $from,
                    display, direction, wrap, gap, padding, width, height, align_items,
                    justify_content, flex_grow, flex_shrink, flex_basis, position, overflow,
                    background, background_image, color, border_color, border_width, radius,
                    shadow, opacity, clip, font_size, font_family, font_weight, font_style,
                    line_height, letter_spacing, word_wrap, text_align, cursor, transition,
                )
            };
        }
        macro_rules! fill {
            ($target:expr, $from:expr, $($f:ident),* $(,)?) => {
                $( if $target.$f == d.$f { $target.$f = defaults.$f.clone(); } )*
            };
        }
        macro_rules! follow {
            ($target:expr, $from:expr, $($f:ident),* $(,)?) => {
                $( if $target.$f == $from.$f { $target.$f = self.styles.base.$f.clone(); } )*
            };
        }

        fields!(fill, self.styles.base, ());

        // A state block declares only what it changes, so every field it
        // inherited has to follow the base to its new value.
        let old = self.styles.base.clone();
        for slot in [
            &mut self.styles.hover,
            &mut self.styles.active,
            &mut self.styles.focus,
            &mut self.styles.disabled,
        ] {
            if let Some(s) = slot {
                fields!(follow, s, old);
            }
        }
    }

    /// Queues the declared animations, ignoring unknown names.
    pub fn start_animations(&self, cx: &mut UiCtx<'_>, node: usize) {
        for spec in &self.animations {
            if let Some(kfs) = crate::anim::get(&spec.name) {
                cx.push_animation(node, Animation::new(spec.clone(), kfs));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Opens a node with the given styles.
fn open(cx: &mut UiCtx<'_>, r: &Resolved, tag: &str) -> usize {
    let id = r.id.clone().unwrap_or_else(|| cx.next_id(tag));
    let n = cx.push_node(&id, tag, r.styles.clone());
    if r.disabled {
        cx.push_disabled();
    }
    r.start_animations(cx, n);
    n
}

/// Balances the [`open`] bookkeeping for the current widget.
///
/// The node stays current so that children written inside `{ ... }` attach to
/// it; [`UiCtx::end`] restores the parent when the block closes.
fn finish(cx: &mut UiCtx<'_>, r: &Resolved, _n: usize) {
    if r.disabled {
        cx.pop_disabled();
    }
}

/// A plain layout container.
fn container(cx: &mut UiCtx<'_>, r: &Resolved, tag: &str) {
    let n = open(cx, r, tag);
    finish(cx, r, n);
}

/// A `Column`: children stacked vertically.
fn column(cx: &mut UiCtx<'_>, r: &mut Resolved) {
    let mut d = Style::default();
    d.direction = Direction::Column;
    r.merge_defaults(&d);
    container(cx, r, "column");
}

/// A `Row`: children side by side.
fn row(cx: &mut UiCtx<'_>, r: &mut Resolved) {
    let mut d = Style::default();
    d.direction = Direction::Row;
    r.merge_defaults(&d);
    container(cx, r, "row");
}

/// A `Label`: text sized to its content.
fn label(cx: &mut UiCtx<'_>, r: &mut Resolved, text: &str) {
    let mut d = Style::default();
    d.justify_content = Justify::Center;
    d.align_items = Align::Center;
    d.flex_shrink = 0.0;
    r.merge_defaults(&d);

    let n = open(cx, r, "label");
    cx.set_text(n, text.to_string(), false);
    cx.set_measure_text(n, true);
    finish(cx, r, n);
}

/// A `Button`.
fn button(cx: &mut UiCtx<'_>, r: &mut Resolved, spec: &WidgetSpec<'_>) {
    let mut d = Style::default();
    d.justify_content = Justify::Center;
    d.align_items = Align::Center;
    d.cursor = CursorKind::Pointer;
    d.background = Some(Color::rgb(64, 82, 122, 1.0));
    d.color = Color::WHITE;
    d.radius = Corners::splat(8.0);
    d.padding = Edges::splat(Dim::Px(9.0));
    d.font_weight = 500;
    // A button keeps its size; it never gets squashed by a flexible sibling.
    d.flex_shrink = 0.0;
    r.merge_defaults(&d);

    let text = spec.label();
    let n = open(cx, r, "button");
    cx.set_text(n, text, false);
    cx.set_measure_text(n, true);
    cx.set_interaction(n, InteractionKind::Button);

    if let Some(link) = spec.any_str(&["link", "href", "url", "open"]) {
        cx.register_link(n, link);
    }
    finish(cx, r, n);
}

/// An `Image`.
fn image(cx: &mut UiCtx<'_>, r: &Resolved, spec: &WidgetSpec<'_>) {
    let source = spec
        .any_str(&["src", "image", "path", "icon", "source"])
        .unwrap_or("")
        .to_string();
    let kind = if source.trim_start().starts_with('<') {
        ImageKind::Svg
    } else {
        ImageKind::Path
    };
    let fit = spec.any_str(&["fit", "mode"]).and_then(Fit::parse).unwrap_or(Fit::Contain);
    let tint = spec
        .any_str(&["tint", "color"])
        .and_then(parse_color)
        .unwrap_or(Color::WHITE);

    let n = open(cx, r, "image");
    cx.set_image(n, source, kind, fit, tint);
    finish(cx, r, n);
}

/// A `Progress` bar.
fn progress(cx: &mut UiCtx<'_>, r: &Resolved, spec: &WidgetSpec<'_>) {
    let value = spec.num_or("value", 0.0).clamp(0.0, 1.0);

    let mut d = Style::default();
    // A bar shares whatever space is left rather than claiming the whole row.
    d.flex_grow = 1.0;
    d.flex_basis = Some(Dim::Px(0.0));
    d.height = Dim::Px(8.0);
    d.radius = Corners::splat(999.0);
    d.background = Some(Color::rgb(44, 52, 70, 1.0));
    d.overflow = Overflow::Hidden;
    d.flex_shrink = 0.0;
    let mut rr = r.clone();
    rr.merge_defaults(&d);

    let outer = open(cx, &rr, "progress");

    // The filled part is a child sized as a percentage of the track.
    let mut fill = Style::default();
    fill.width = Dim::Pct(value * 100.0);
    fill.height = Dim::Pct(100.0);
    fill.radius = Corners::splat(999.0);
    fill.background = Some(
        spec.any_str(&["color", "fill"]).and_then(parse_color).unwrap_or(Color::rgb(110, 190, 255, 1.0)),
    );
    let fill_id = cx.next_id("progress-fill");
    let inner = Resolved::of(fill);
    let f = cx.push_node(&fill_id, "progress-fill", inner.styles.clone());
    finish(cx, &rr, outer);
    finish(cx, &inner, f);
}

/// A `Checkbox` with an optional label.
fn checkbox(cx: &mut UiCtx<'_>, r: &Resolved, spec: &WidgetSpec<'_>) {
    let label_text = spec.label();

    let mut d = Style::default();
    d.direction = Direction::Row;
    d.align_items = Align::Center;
    d.gap = Dim::Px(8.0);
    d.cursor = CursorKind::Pointer;
    d.flex_shrink = 0.0;
    let mut rr = r.clone();
    rr.merge_defaults(&d);

    // The `id` belongs to the box, which is what holds the value, so the
    // wrapper keeps a generated one.
    let mut wrapper = rr.clone();
    wrapper.id = None;
    let n = open(cx, &wrapper, "checkbox");

    let box_node = {
        let accent = spec.str("color").and_then(parse_color).unwrap_or(Color::rgb(74, 127, 255, 1.0));
        let id = rr.id.clone().unwrap_or_default();
        let on = cx.state().get_bool(&id, spec.flag("checked"));

        let mut b = Style::default();
        b.width = Dim::Px(18.0);
        b.height = Dim::Px(18.0);
        b.radius = Corners::splat(5.0);
        b.border_width = 1.5;
        b.border_color = if on { accent } else { Color::rgb(96, 112, 146, 1.0) };
        b.background = Some(if on { accent } else { Color::rgb(28, 34, 50, 1.0) });
        b.justify_content = Justify::Center;
        b.align_items = Align::Center;
        b.overflow = Overflow::Visible;
        let mut inner = Resolved::of(b);
        // The box carries the checkbox's `id`, so its on/off value is stored
        // under the name the app wrote.
        inner.id = rr.id.clone();
        let bn = open(cx, &inner, "checkbox-box");
        cx.set_interaction(bn, InteractionKind::Checkbox);

        if on {
            // Two rotated bars make the tick.
            for (len, dx, dy, deg) in [(5.0f32, -2.0f32, 1.5f32, -45.0f32), (9.0, 2.0, -1.0, 45.0)] {
                let mut s = Style::default();
                s.position = Position::Absolute;
                s.width = Dim::Px(2.0);
                s.height = Dim::Px(len);
                s.radius = Corners::splat(1.0);
                s.background = Some(Color::WHITE);
                s.rotate = deg.to_radians();
                s.inset = Edges {
                    top: Dim::Px(9.0 + dy),
                    right: Dim::Px(0.0),
                    bottom: Dim::Px(0.0),
                    left: Dim::Px(9.0 + dx),
                };
                let tick = Resolved::of(s);
                let tick_id = cx.next_id("checkbox-tick");
                let t = cx.push_node(&tick_id, "checkbox-tick", tick.styles.clone());
                finish(cx, &tick, t);
            }
        }

        bn
    };

    if !label_text.is_empty() {
        // The label is a sibling of the box, not one of its children.
        cx.close_node(box_node);
        let mut ls = Style::default();
        ls.flex_shrink = 0.0;
        let inner = Resolved::of(ls);
        let ln = open(cx, &inner, "label");
        cx.set_text(ln, label_text, false);
        cx.set_measure_text(ln, true);
        finish(cx, &inner, ln);
    }

    finish(cx, &wrapper, n);
    finish(cx, &Resolved::blank(), box_node);
}

/// A `Slider`.
fn slider(cx: &mut UiCtx<'_>, r: &Resolved, spec: &WidgetSpec<'_>) {
    let value = spec.num_or("value", 0.5).clamp(0.0, 1.0);

    let mut d = Style::default();
    d.direction = Direction::Column;
    d.width = Dim::Pct(100.0);
    d.height = Dim::Px(22.0);
    d.justify_content = Justify::Center;
    d.cursor = CursorKind::Pointer;
    d.flex_shrink = 0.0;
    let mut rr = r.clone();
    rr.merge_defaults(&d);

    let n = open(cx, &rr, "slider");
    cx.set_interaction(n, InteractionKind::Slider);

    let track = {
        let mut t = Style::default();
        t.width = Dim::Pct(100.0);
        t.height = Dim::Px(6.0);
        t.radius = Corners::splat(999.0);
        t.background = Some(Color::rgb(44, 52, 70, 1.0));
        let inner = Resolved::of(t);
        open(cx, &inner, "slider-track")
    };
    // Track, fill and knob are siblings; only the knob is absolutely placed.
    cx.close_node(track);

    // The filled part is a sibling of the track, sized as a percentage of it.
    {
        let mut f = Style::default();
        f.width = Dim::Pct(value * 100.0);
        f.height = Dim::Pct(100.0);
        f.radius = Corners::splat(999.0);
        f.background = Some(Color::rgb(120, 200, 255, 1.0));
        let inner = Resolved::of(f);
        let fnode = open(cx, &inner, "slider-fill");
        finish(cx, &inner, fnode);
        cx.close_node(fnode);
    };

    let knob = {
        let mut k = Style::default();
        k.position = Position::Absolute;
        k.width = Dim::Px(16.0);
        k.height = Dim::Px(16.0);
        k.radius = Corners::splat(999.0);
        k.background = Some(Color::WHITE);
        k.shadow = BoxShadow { offset: Vec2::new(0.0, 2.0), blur: 6.0, color: Color::rgb(0, 0, 0, 0.35) };
        // Centre the knob on the value position.
        k.inset = Edges {
            top: Dim::Px(-5.0),
            right: Dim::Px(-8.0),
            bottom: Dim::Px(-5.0),
            left: Dim::Pct(value * 100.0),
        };
        let inner = Resolved::of(k);
        let knode = open(cx, &inner, "slider-knob");
        finish(cx, &inner, knode);
        knode
    };

    finish(cx, &Resolved::blank(), knob);
    finish(cx, &rr, n);
    finish(cx, &Resolved::blank(), track);
}

/// A `TextInput`.
fn text_input(cx: &mut UiCtx<'_>, r: &Resolved, spec: &WidgetSpec<'_>) {
    let placeholder = spec.str("placeholder").unwrap_or("").to_string();
    let initial = spec.any_str(&["value", "text"]).unwrap_or("").to_string();

    let mut d = Style::default();
    d.height = Dim::Px(34.0);
    d.padding = Edges {
        top: Dim::Px(6.0),
        right: Dim::Px(10.0),
        bottom: Dim::Px(6.0),
        left: Dim::Px(10.0),
    };
    d.radius = Corners::splat(8.0);
    d.background = Some(Color::rgb(22, 28, 42, 1.0));
    d.border_width = 1.0;
    d.border_color = Color::rgb(56, 66, 90, 1.0);
    d.color = Color::WHITE;
    d.cursor = CursorKind::Text;
    d.flex_shrink = 0.0;
    let mut rr = r.clone();
    rr.merge_defaults(&d);

    let n = open(cx, &rr, "text-input");
    cx.set_interaction(n, InteractionKind::TextInput);

    let value = cx.state().get_text(&rr.id.clone().unwrap_or_default(), &initial);
    let showing_placeholder = value.is_empty();
    let shown = if showing_placeholder { placeholder } else { value };
    cx.set_text(n, shown, showing_placeholder);
    // The text still has to be shaped: an unmeasured node draws no glyphs.
    cx.set_measure_text(n, true);

    finish(cx, &rr, n);
}

/// A `Spacer`: flexible empty space.
fn spacer(cx: &mut UiCtx<'_>, r: &Resolved) {
    let mut d = Style::default();
    d.flex_grow = 1.0;
    d.flex_basis = Some(Dim::Px(0.0));
    let mut rr = r.clone();
    rr.merge_defaults(&d);
    container(cx, &rr, "spacer");
}

/// A `Divider`.
fn divider(cx: &mut UiCtx<'_>, r: &Resolved) {
    let mut d = Style::default();
    d.height = Dim::Px(1.0);
    d.background = Some(Color::rgb(52, 62, 84, 1.0));
    d.flex_shrink = 0.0;
    let mut rr = r.clone();
    rr.merge_defaults(&d);
    container(cx, &rr, "divider");
}

/// Routes a tag to its implementation.
pub fn dispatch(cx: &mut UiCtx<'_>, tag: &str, spec: &WidgetSpec<'_>, resolved: Resolved) {
    let mut r = resolved;
    match tag {
        "column" | "col" | "vstack" | "stack-column" => column(cx, &mut r),
        "row" | "hbox" | "strip" => row(cx, &mut r),
        "box" | "div" | "node" | "panel" | "card" => container(cx, &r, "box"),
        "label" | "text" | "p" | "span" => {
            let text = spec.label();
            label(cx, &mut r, &text);
        }
        "title" | "h1" | "heading" => {
            let text = spec.label();
            let mut d = Style::default();
            d.font_size = 24.0;
            d.font_weight = 700;
            r.merge_defaults(&d);
            label(cx, &mut r, &text);
        }
        "button" | "btn" => button(cx, &mut r, spec),
        "image" | "img" | "icon" | "svg" | "png" | "pic" => image(cx, &r, spec),
        "progress" | "progressbar" | "bar" => progress(cx, &r, spec),
        "checkbox" | "check" | "toggle" => checkbox(cx, &r, spec),
        "slider" | "range" => slider(cx, &r, spec),
        "textinput" | "input" | "field" | "edit" => text_input(cx, &r, spec),
        "spacer" | "gap" | "space" | "fill" => spacer(cx, &r),
        "divider" | "hr" | "rule" | "separator" => divider(cx, &r),
        _ => {
            // Unknown tags behave like a generic box: styling still applies.
            container(cx, &r, tag);
        }
    }
}

/// Normalizes a tag for matching.
pub fn normalize_tag(tag: &str) -> String {
    tag.to_ascii_lowercase()
}

/// Builds text attributes from a style.
pub fn attrs_from_style(style: &Style) -> TextAttrs {
    TextAttrs {
        size: style.font_size,
        line_height: style.line_height,
        family: style.font_family.clone(),
        weight: style.font_weight,
        italic: style.font_style == crate::style::FontStyle::Italic,
        letter_spacing: style.letter_spacing,
        align: style.text_align,
        wrap: style.word_wrap,
        width: None,
    }
}

/// Key that activates a focused button.
pub const ACTIVATE_KEY: Key = Key::Enter;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attr_conversions() {
        assert_eq!(Attr::Str("x".into()).to_text(), "x");
        assert_eq!(Attr::Num(2.0).to_text(), "2");
        assert_eq!(Attr::Num(2.5).to_text(), "2.5");
        assert_eq!(Attr::Bool(true).as_bool(), Some(true));
        assert_eq!(Attr::Ident("true".into()).as_bool(), Some(true));
        assert_eq!(Attr::Num(3.0).as_num(), Some(3.0));
    }

    #[test]
    fn tags_normalize() {
        assert_eq!(normalize_tag("Button"), "button");
        assert_eq!(normalize_tag("BUTTON"), "button");
    }

    #[test]
    fn defaults_do_not_override_user_values() {
        let mut r = Resolved::blank();
        r.styles.base = Style::parse("padding: 3px;");
        let mut d = Style::default();
        d.padding = Edges::splat(Dim::Px(9.0));
        d.color = Color::WHITE;
        r.merge_defaults(&d);

        // The user's padding survives; the default colour fills in.
        assert_eq!(r.styles.base.padding.left, Dim::Px(3.0));
        assert_eq!(r.styles.base.color, Color::WHITE);
    }

    #[test]
    fn state_detection() {
        assert_eq!(Style::state_of("on-hover"), Some(State::Hover));
        assert_eq!(Style::state_of("on_hover"), Some(State::Hover));
        assert_eq!(Style::state_of(":active"), Some(State::Active));
        assert_eq!(Style::state_of("disabled"), Some(State::Disabled));
        assert_eq!(Style::state_of("padding"), None);
    }
}