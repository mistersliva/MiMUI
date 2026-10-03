//! The CSS-like layer: a declaration-block parser and one table mapping
//! property names to style fields.
//!
//! ```
//! use mimui::prelude::*;
//!
//! let s = Style::parse("padding: 8px 12px; radius: 10px;");
//! assert_eq!(s.padding.left, Dim::Px(12.0));
//! ```
//!
//! Unknown properties are ignored, because the same block syntax is used for
//! `on-hover:` and friends. Use [`apply_block_checked`] to surface them.

use crate::anim::{self, Transition};
use crate::color::{parse_color, Color};
use crate::easing::{parse_easing, Easing};
use crate::geom::{Corners, Dim, Edges, Vec2};
use crate::style::{
    Align, BgFit, BoxShadow, CursorKind, Direction, Display, Filter, FontStyle, Justify,
    Overflow, Position, Style, TextAlign, WrapMode,
};

/// Error from a declaration that could not be understood.
#[derive(Clone, Debug, PartialEq)]
pub struct CssError {
    pub property: String,
    pub value: String,
}

impl std::fmt::Display for CssError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "`{}: {}` is not a valid MiMUI property", self.property, self.value)
    }
}

/// Applies a whole declaration block, ignoring anything it does not know.
pub fn apply_block(style: &mut Style, src: &str) {
    for (prop, val) in parse_block(src) {
        let _ = apply_property(style, &prop, &val);
    }
}

/// Like [`apply_block`] but reports unknown properties.
pub fn apply_block_checked(style: &mut Style, src: &str) -> Vec<CssError> {
    let mut errs = Vec::new();
    for (prop, val) in parse_block(src) {
        if let Err(e) = apply_property(style, &prop, &val) {
            errs.push(e);
        }
    }
    errs
}

/// Splits `"a: b; c: d;"` into property/value pairs, tolerating missing
/// semicolons and `//` comments.
pub fn parse_block(src: &str) -> Vec<(String, String)> {
    let cleaned = strip_comments(src);
    let mut out = Vec::new();
    for part in cleaned.split(';') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        match part.find(':') {
            Some(i) => {
                let p = part[..i].trim().to_ascii_lowercase();
                let v = part[i + 1..].trim().to_string();
                if !p.is_empty() {
                    out.push((p, v));
                }
            }
            None => out.push((part.to_ascii_lowercase(), String::new())),
        }
    }
    out
}

fn strip_comments(s: &str) -> String {
    if !s.contains("//") {
        return s.to_string();
    }
    s.lines().map(|l| match l.find("//") { Some(i) => &l[..i], None => l }).collect::<Vec<_>>().join("\n")
}

// ---------------------------------------------------------------------------
// value parsing
// ---------------------------------------------------------------------------

/// Strips a trailing unit, returning the numeric part. `rem`/`pt` map onto px.
pub fn parse_num(s: &str) -> Option<f32> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let t = t.strip_suffix('%').map(|v| (v, true)).unwrap_or((t, false));
    let (body, percent) = t;
    let body = body.trim();
    let body = body
        .strip_suffix("px")
        .or_else(|| body.strip_suffix("pt"))
        .or_else(|| body.strip_suffix("dp"))
        .or_else(|| body.strip_suffix("rem"))
        .or_else(|| body.strip_suffix("em"))
        .unwrap_or(body);
    body.trim().parse::<f32>().ok().map(|v| if percent { v / 100.0 } else { v })
}

/// Parses a length list like `10px 20% 4`.
pub fn parse_dims(s: &str) -> Option<Vec<Dim>> {
    let mut out = Vec::new();
    for p in s.split_whitespace() {
        out.push(parse_dim(p)?);
    }
    Some(out)
}

/// One length: `12`, `12px`, `50%`, `auto`.
pub fn parse_dim(s: &str) -> Option<Dim> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    if t.eq_ignore_ascii_case("auto") || t.eq_ignore_ascii_case("none") {
        return Some(Dim::Auto);
    }
    if t.ends_with('%') {
        return parse_num(t).map(|v| Dim::Pct(v * 100.0));
    }
    parse_num(t).map(Dim::Px)
}

/// `padding: 8px 12px` → all four edges.
pub fn parse_edges(s: &str) -> Option<Edges<Dim>> {
    Some(Edges::from_slice(&parse_dims(s)?))
}

pub fn parse_len(s: &str) -> Option<f32> {
    parse_num(s)
}

/// `radius: 10px 20px / 4px` — corner radii with optional ellipse shorthand.
pub fn parse_radius(s: &str) -> Option<Corners<f32>> {
    let (h, v) = match s.split_once('/') {
        Some((a, b)) => (a, b),
        None => (s, ""),
    };
    let hs = split_len(h);
    if hs.is_empty() {
        return None;
    }
    // `10 / 4` -> vertical only for top/bottom, horizontal for left/right.
    let mut corners = match hs.len() {
        1 => Corners::splat(hs[0]),
        2 => Corners { tl: hs[0], tr: hs[1], br: hs[0], bl: hs[1] },
        3 => Corners { tl: hs[0], tr: hs[1], br: hs[2], bl: hs[1] },
        _ => Corners { tl: hs[0], tr: hs[1], br: hs[2], bl: hs[3] },
    };
    if !v.trim().is_empty() {
        let vs = split_len(v);
        let hx = hs.clone();
        let set = |c: &mut f32, vert: bool, i: usize| {
            let from = if vert { &vs } else { &hx };
            if let Some(val) = from.get(i.min(from.len().saturating_sub(1))) {
                *c = *val;
            }
        };
        set(&mut corners.tl, true, 0);
        set(&mut corners.tr, true, 1);
        set(&mut corners.br, true, 2);
        set(&mut corners.bl, true, 3);
    }
    for c in [corners.tl, corners.tr, corners.br, corners.bl] {
        if c < 0.0 {
            return None;
        }
    }
    Some(corners)
}

/// Splits `"10px 2"` into numbers, tolerating a unit only on the first.
fn split_len(s: &str) -> Vec<f32> {
    s.split_whitespace().filter_map(parse_num).collect()
}

fn parse_f32(s: &str) -> Option<Vec<f32>> {
    Some(s.split_whitespace().filter_map(parse_num).collect())
}

pub fn parse_bool(s: &str) -> Option<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" | "none" => Some(false),
        _ => None,
    }
}

fn kw(s: &str) -> String {
    s.trim().to_ascii_lowercase()
}

fn one_of(s: &str, table: &[(&str, u8)]) -> Option<u8> {
    let k = kw(s);
    table.iter().find(|(n, _)| *n == k).map(|(_, v)| *v)
}

const ALIGN_KWS: &[(&str, u8)] = &[
    ("start", 0),
    ("flex-start", 0),
    ("left", 0),
    ("center", 1),
    ("middle", 1),
    ("end", 2),
    ("flex-end", 2),
    ("right", 2),
    ("stretch", 3),
    ("baseline", 4),
    ("normal", 0),
];

const JUSTIFY_KWS: &[(&str, u8)] = &[
    ("start", 0),
    ("flex-start", 0),
    ("left", 0),
    ("center", 1),
    ("middle", 1),
    ("end", 2),
    ("flex-end", 2),
    ("right", 2),
    ("space-between", 3),
    ("between", 3),
    ("space-around", 4),
    ("around", 4),
    ("space-evenly", 5),
    ("evenly", 5),
];

fn align_of(s: &str) -> Option<Align> {
    Some(match one_of(s, ALIGN_KWS)? {
        1 => Align::Center,
        2 => Align::End,
        3 => Align::Stretch,
        _ => Align::Start,
    })
}

fn justify_of(s: &str) -> Option<Justify> {
    Some(match one_of(s, JUSTIFY_KWS)? {
        1 => Justify::Center,
        2 => Justify::End,
        3 => Justify::SpaceBetween,
        4 => Justify::SpaceAround,
        5 => Justify::SpaceEvenly,
        _ => Justify::Start,
    })
}

/// `weight: 600` | `bold` | `semibold` | `lighter` | `bolder`
pub fn parse_font_weight(s: &str) -> Option<u16> {
    let k = kw(s);
    Some(match k.as_str() {
        "thin" => 100,
        "hairline" => 100,
        "light" => 300,
        "normal" | "regular" | "book" => 400,
        "medium" => 500,
        "semibold" | "demibold" => 600,
        "bold" => 700,
        "extrabold" | "ultrabold" => 800,
        "heavy" | "black" => 900,
        "lighter" => 300,
        "bolder" => 700,
        _ => k.parse().ok()?,
    })
}

fn parse_filter(s: &str) -> Option<Filter> {
    let k = kw(s);
    let (name, amt) = match k.split_once('(') {
        Some((n, r)) => (n.to_string(), Some(r.trim_end_matches(')').trim().to_string())),
        None => (k.clone(), None),
    };
    let v = || amt.as_deref().and_then(parse_num).unwrap_or(1.0);
    Some(match name.as_str() {
        "none" => Filter::None,
        "blur" => Filter::Blur(v().max(0.0)),
        "brightness" => Filter::Brightness(v()),
        "grayscale" => Filter::Grayscale(v()),
        "sepia" => Filter::Sepia(v()),
        "invert" => Filter::Invert(v()),
        "saturate" => Filter::Saturate(v()),
        "hue-rotate" | "huerotate" => Filter::HueRotate(v()),
        _ => return None,
    })
}

fn parse_cursor(s: &str) -> Option<CursorKind> {
    Some(match kw(s).as_str() {
        "default" | "arrow" => CursorKind::Default,
        "pointer" | "hand" | "link" => CursorKind::Pointer,
        "text" | "i-beam" => CursorKind::Text,
        "grab" => CursorKind::Grab,
        "grabbing" => CursorKind::Grabbing,
        "not-allowed" | "forbidden" => CursorKind::NotAllowed,
        "crosshair" => CursorKind::Crosshair,
        "move" => CursorKind::Move,
        "wait" => CursorKind::Wait,
        "resize" | "ew-resize" | "ns-resize" => CursorKind::Resize,
        _ => return None,
    })
}

/// `box-shadow: 0 6px 16px #0008` (offset-x offset-y blur color).
fn parse_shadow(s: &str) -> Option<BoxShadow> {
    let t = s.trim();
    if kw(t) == "none" {
        return Some(BoxShadow::NONE);
    }
    let mut nums = Vec::new();
    let mut color = Color::TRANSPARENT;
    for tok in t.split_whitespace() {
        if tok.contains('#') || kw(tok).contains("rgb") {
            if let Some(c) = parse_color(tok) {
                color = c;
                continue;
            }
        }
        // Bare numbers are lengths; a bare word is a color keyword.
        if parse_num(tok).is_some() {
            nums.push(parse_num(tok).expect("checked"));
        } else if let Some(c) = parse_color(tok) {
            color = c;
        } else {
            return None;
        }
    }
    let (ox, oy) = match nums.len() {
        0 => (0.0, 0.0),
        1 => (nums[0], nums[0]),
        _ => (nums[0], nums[1]),
    };
    let blur = nums.get(2).copied().unwrap_or(0.0);
    Some(BoxShadow { offset: Vec2::new(ox, oy), blur, color })
}

// ---------------------------------------------------------------------------
// property table
// ---------------------------------------------------------------------------

/// Applies a single `property: value` pair.
pub fn apply_property(s: &mut Style, prop: &str, value: &str) -> Result<(), CssError> {
    let bad = || CssError { property: prop.to_string(), value: value.to_string() };
    // Shorthand first: these eat a lot of values.
    if is_prop(prop, &["padding"]) {
        s.padding = parse_edges(value).ok_or_else(bad)?;
        return Ok(());
    }
    if is_prop(prop, &["inset"]) {
        s.inset = parse_edges(value).ok_or_else(bad)?;
        return Ok(());
    }
    if is_prop(prop, &["gap-x"]) {
        s.gap = parse_edges(value).ok_or_else(bad)?.left;
        return Ok(());
    }
    if let Some(v) = parse_edges(value)
        && is_prop(prop, &["margin"])
    {
        s.margin = v;
        return Ok(());
    }

    match prop {
        // --- box ---
        "display" => s.display = if kw(value) == "none" { Display::None } else { Display::Flex },
        "position" => s.position = if kw(value) == "absolute" { Position::Absolute } else { Position::Relative },
        "overflow" => {
            s.overflow = match kw(value).as_str() {
                "hidden" | "clip" => Overflow::Hidden,
                "scroll" | "auto" => Overflow::Scroll,
                "visible" => Overflow::Visible,
                _ => return Err(bad()),
            }
        }
        "visible" => s.visible = parse_bool(value).ok_or_else(bad)?,
        "z-index" => s.z_index = parse_num(value).ok_or_else(bad)? as i32,

        // --- flex ---
        "flex-direction" | "direction" => {
            s.direction = match kw(value).as_str() {
                "row" | "horizontal" => Direction::Row,
                "column" | "vertical" => Direction::Column,
                _ => return Err(bad()),
            }
        }
        "flex-wrap" | "wrap" => {
            s.wrap = if kw(value) == "wrap" { WrapMode::Wrap } else { WrapMode::NoWrap }
        }
        "gap" | "row-gap" | "column-gap" => s.gap = parse_dim(value).ok_or_else(bad)?,
        "align-items" | "align" => s.align_items = align_of(value).ok_or_else(bad)?,
        "justify-content" | "justify" => s.justify_content = justify_of(value).ok_or_else(bad)?,
        "align-self" => s.align_self = Some(align_of(value).ok_or_else(bad)?),
        "justify-self" => s.justify_self = Some(justify_of(value).ok_or_else(bad)?),
        "flex-grow" => s.flex_grow = parse_num(value).ok_or_else(bad)?.max(0.0),
        "flex-shrink" => s.flex_shrink = parse_num(value).ok_or_else(bad)?.max(0.0),
        "flex-basis" => s.flex_basis = Some(parse_dim(value).ok_or_else(bad)?),
        "flex" => {
            // `flex: 1` -> grow 1, shrink 1, basis zero, so siblings share the
            // space evenly no matter how wide their content is.
            let parts: Vec<&str> = value.split_whitespace().collect();
            match parts.len() {
                1 => {
                    s.flex_grow = parse_num(parts[0]).ok_or_else(bad)?;
                    s.flex_shrink = 1.0;
                    s.flex_basis = Some(Dim::ZERO);
                }
                _ => {
                    s.flex_shrink = parse_num(parts[0]).ok_or_else(bad)?;
                    s.flex_grow = parse_num(parts[1]).ok_or_else(bad)?;
                    if parts.len() > 2 {
                        s.flex_basis = Some(parse_dim(parts[2]).ok_or_else(bad)?);
                    }
                }
            }
        }

        // --- size ---
        "width" | "w" => s.width = parse_dim(value).ok_or_else(bad)?,
        "height" | "h" => s.height = parse_dim(value).ok_or_else(bad)?,
        "size" => {
            let d = parse_dims(value).ok_or_else(bad)?;
            s.width = *d.first().ok_or_else(bad)?;
            s.height = *d.get(1).unwrap_or(d.first().ok_or_else(bad)?);
        }
        "min-width" => s.min_width = Some(parse_dim(value).ok_or_else(bad)?),
        "max-width" => s.max_width = Some(parse_dim(value).ok_or_else(bad)?),
        "min-height" => s.min_height = Some(parse_dim(value).ok_or_else(bad)?),
        "max-height" => s.max_height = Some(parse_dim(value).ok_or_else(bad)?),

        // --- edges ---
        "padding-top" | "padding-right" | "padding-bottom" | "padding-left" => {
            let d = parse_dim(value).ok_or_else(bad)?;
            edge_set(&mut s.padding, prop.strip_prefix("padding-").unwrap(), d);
        }
        "margin-top" | "margin-right" | "margin-bottom" | "margin-left" => {
            let d = parse_dim(value).ok_or_else(bad)?;
            edge_set(&mut s.margin, prop.strip_prefix("margin-").unwrap(), d);
        }
        "top" | "right" | "bottom" | "left" => {
            let d = parse_dim(value).ok_or_else(bad)?;
            edge_set(&mut s.inset, prop, d);
        }

        // --- visuals ---
        "background" | "background-color" | "bg" => {
            s.background = Some(parse_color(value).ok_or_else(bad)?);
        }
        "background-image" | "image" => {
            s.background_image = Some(unquote(value));
        }
        "background-size" | "bg-size" => {
            s.background_fit = match kw(value).as_str() {
                "cover" => BgFit::Cover,
                "contain" => BgFit::Contain,
                "fill" | "100% 100%" | "stretch" => BgFit::Fill,
                "none" | "auto" => BgFit::None_,
                _ => return Err(bad()),
            }
        }
        "color" | "foreground" | "fg" | "text-color" => s.color = parse_color(value).ok_or_else(bad)?,
        "border-color" => s.border_color = parse_color(value).ok_or_else(bad)?,
        "border-width" | "border" => {
            let d = parse_dim(value).ok_or_else(bad)?;
            s.border_width = d.resolve_or(0.0, 0.0);
            if let Some(c) = parse_color(value) {
                s.border_color = c;
            }
        }
        "radius" | "border-radius" => s.radius = parse_radius(value).ok_or_else(bad)?,
        "shadow" | "box-shadow" => s.shadow = parse_shadow(value).ok_or_else(bad)?,
        "shadow-blur" => s.shadow.blur = parse_num(value).ok_or_else(bad)?.max(0.0),
        "shadow-color" => s.shadow.color = parse_color(value).ok_or_else(bad)?,
        "opacity" => s.opacity = parse_num(value).ok_or_else(bad)?.clamp(0.0, 1.0),
        "clip" => s.clip = parse_bool(value).ok_or_else(bad)?,
        "filter" => s.filter = parse_filter(value).ok_or_else(bad)?,
        "blur" => s.filter = Filter::Blur(parse_num(value).ok_or_else(bad)?.max(0.0)),

        // --- text ---
        "font-size" => s.font_size = parse_num(value).ok_or_else(bad)?.max(1.0),
        "font-family" | "font" => s.font_family = unquote(value),
        "font-weight" => s.font_weight = parse_font_weight(value).ok_or_else(bad)?,
        "font-style" => {
            s.font_style = if kw(value) == "italic" { FontStyle::Italic } else { FontStyle::Normal }
        }
        "line-height" => {
            s.line_height = if value.trim().ends_with('%') {
                parse_num(value).ok_or_else(bad)? * 100.0
            } else {
                parse_num(value).ok_or_else(bad)?
            };
        }
        "letter-spacing" => s.letter_spacing = parse_num(value).ok_or_else(bad)?,
        "word-wrap" => s.word_wrap = parse_bool(value).ok_or_else(bad)?,
        "text-align" => {
            s.text_align = match kw(value).as_str() {
                "left" | "start" => TextAlign::Left,
                "center" | "centre" => TextAlign::Center,
                "right" | "end" => TextAlign::Right,
                _ => return Err(bad()),
            }
        }

        // --- transform ---
        "translate" => {
            let v = parse_f32(value).ok_or_else(bad)?;
            s.translate = match v.len() {
                0 => Vec2::ZERO,
                1 => Vec2::splat(v[0]),
                _ => Vec2::new(v[0], v[1]),
            };
        }
        "translate-x" => s.translate.x = parse_num(value).ok_or_else(bad)?,
        "translate-y" => s.translate.y = parse_num(value).ok_or_else(bad)?,
        "scale" => {
            let v = parse_f32(value).ok_or_else(bad)?;
            match v.len() {
                0 => s.scale = Vec2::ONE,
                1 => s.scale = Vec2::splat(v[0]),
                _ => s.scale = Vec2::new(v[0], v[1]),
            };
        }
        "scale-x" => s.scale.x = parse_num(value).ok_or_else(bad)?,
        "scale-y" => s.scale.y = parse_num(value).ok_or_else(bad)?,
        "rotate" | "rotation" => s.rotate = f32::to_radians(parse_num(value).ok_or_else(bad)?),

        // --- interaction ---
        "cursor" => s.cursor = parse_cursor(value).ok_or_else(bad)?,
        "transition" => {
            s.transition = anim::parse_transition(value).ok_or_else(bad)?;
        }
        "ease" | "easing" => {
            s.transition.ease = parse_easing(value).ok_or_else(bad)?;
        }

        // --- motion (kept for symmetry; `animation:` is handled by the cascade) ---
        "transition-duration" => {
            s.transition.duration = parse_num(value).ok_or_else(bad)?;
        }
        "transition-delay" => s.transition.delay = parse_num(value).ok_or_else(bad)?,

        _ => return Err(bad()),
    }
    Ok(())
}

fn edge_set<T>(edges: &mut Edges<T>, side: &str, v: T) {
    match side {
        "top" => edges.top = v,
        "right" => edges.right = v,
        "bottom" => edges.bottom = v,
        "left" => edges.left = v,
        _ => {}
    }
}

fn is_prop(p: &str, names: &[&str]) -> bool {
    names.contains(&p)
}

fn unquote(s: &str) -> String {
    let t = s.trim();
    t.trim_matches(|c| c == '"' || c == '\'').to_string()
}

/// `Ease` re-export so callers can build a `Transition` without importing
/// [`crate::easing`].
pub type TransitionEasing = Easing;

/// Convenience for `transition` defaults in builders.
pub fn transition_of(s: &str) -> Transition {
    anim::parse_transition(s).unwrap_or_default()
}

/// Every property [`apply_property`] understands.
///
/// Attributes written inline on a widget are matched against this list, so it
/// must stay in sync with the match arms above.
const PROPERTIES: &[&str] = &[
    "display", "position", "overflow", "visible", "z-index",
    "direction", "flex-direction", "wrap", "flex-wrap", "gap", "row-gap", "column-gap",
    "align", "align-items", "justify", "justify-content", "align-self", "justify-self",
    "flex-grow", "flex-shrink", "flex-basis", "flex",
    "width", "w", "height", "h", "size",
    "min-width", "max-width", "min-height", "max-height",
    "padding", "padding-top", "padding-right", "padding-bottom", "padding-left", "gap-x",
    "margin", "margin-top", "margin-right", "margin-bottom", "margin-left",
    "top", "right", "bottom", "left", "inset",
    "background", "background-color", "bg", "background-image", "image", "background-size",
    "bg-size", "color", "foreground", "fg", "text-color",
    "border-color", "border-width", "border", "radius", "border-radius",
    "shadow", "box-shadow", "shadow-blur", "shadow-color",
    "opacity", "clip", "filter", "blur",
    "font-size", "font-family", "font", "font-weight", "font-style", "line-height",
    "letter-spacing", "word-wrap", "text-align",
    "translate", "translate-x", "translate-y", "scale", "scale-x", "scale-y",
    "rotate", "rotation", "cursor",
    "transition", "transition-duration", "transition-delay", "ease", "easing",
];

/// True when `name` is a style property, so it can be written inline on a widget.
pub fn is_property(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    PROPERTIES.contains(&lower.as_str()) || crate::style::Style::state_of(&lower).is_some()
}

/// Property names understood by [`apply_property`], for tooling and docs.
pub fn known_properties() -> Vec<&'static str> {
    PROPERTIES.to_vec()
}

/// Applies a `transition:` list to a style, used by the cascade.
pub fn set_transition(s: &mut Style, spec: &str) -> Result<(), CssError> {
    s.transition = anim::parse_transition(spec)
        .ok_or_else(|| CssError { property: "transition".into(), value: spec.to_string() })?;
    Ok(())
}

/// Applies a `transition: all 0.2s` list where every property is animated.
pub fn transition_all(s: &mut Style, duration: f32) {
    s.transition = Transition {
        duration,
        delay: 0.0,
        ease: Easing::default(),
        props: anim::ALL_PROPS.to_vec(),
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::AnimProp;

    #[test]
    fn parses_a_block() {
        let s = Style::parse(
            "padding: 8px 12px; radius: 10px 20px; bg: #123456; font-weight: bold;",
        );
        assert_eq!(s.padding.left, Dim::Px(12.0));
        assert_eq!(s.padding.top, Dim::Px(8.0));
        assert_eq!(s.radius, Corners { tl: 10.0, tr: 20.0, br: 10.0, bl: 20.0 });
        assert_eq!(s.background, Some(Color::rgb(0x12, 0x34, 0x56, 1.0)));
        assert_eq!(s.font_weight, 700);
    }

    #[test]
    fn unknown_property_is_an_error_not_a_panic() {
        let errs = apply_block_checked(&mut Style::default(), "wat: 3; padding: 2px;");
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].property, "wat");
    }

    #[test]
    fn percent_and_auto_dims() {
        let s = Style::parse("width: 50%; height: auto; margin: -4px;");
        assert_eq!(s.width, Dim::Pct(50.0));
        assert!(s.height.is_auto());
        assert_eq!(s.margin.top, Dim::Px(-4.0));
    }

    #[test]
    fn shadow_shorthand() {
        let s = Style::parse("shadow: 0 6px 16px #0008;");
        assert_eq!(s.shadow.offset, Vec2::new(0.0, 6.0));
        assert_eq!(s.shadow.blur, 16.0);
        assert!(s.shadow.color.a < 0.6);
        assert_eq!(Style::parse("shadow: none;").shadow, BoxShadow::NONE);
    }

    #[test]
    fn transition_shorthand() {
        let s = Style::parse("transition: background 0.2s ease-out;");
        assert_eq!(s.transition.duration, 0.2);
        assert!(s.transition.covers(AnimProp::Bg));
    }
}