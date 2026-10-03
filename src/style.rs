//! The style model: one `Style` value per element plus the trigger-specific
//! overrides (`on-hover`, `on-active`, …) that the cascade picks from.

use crate::anim::Transition;
use crate::color::Color;
use crate::geom::{Corners, Dim, Edges, Vec2};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Display {
    #[default]
    Flex,
    None,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    #[default]
    Row,
    Column,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WrapMode {
    #[default]
    NoWrap,
    Wrap,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
    Stretch,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Justify {
    #[default]
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Position {
    #[default]
    Relative,
    Absolute,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Overflow {
    #[default]
    Visible,
    Hidden,
    Scroll,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
}

/// How a background image fills its box.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum BgFit {
    #[default]
    Cover,
    Contain,
    Fill,
    None_,
}

/// A rasterization filter applied to a node and its subtree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Filter {
    None,
    Blur(f32),
    Brightness(f32),
    Grayscale(f32),
    Sepia(f32),
    Invert(f32),
    Saturate(f32),
    HueRotate(f32),
}

impl Default for Filter {
    fn default() -> Self {
        Filter::None
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorKind {
    #[default]
    Default,
    Pointer,
    Text,
    Grab,
    Grabbing,
    NotAllowed,
    Crosshair,
    Move,
    Wait,
    Resize,
}

/// A drop shadow drawn behind the node's background.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxShadow {
    pub offset: Vec2,
    pub blur: f32,
    pub color: Color,
}

impl BoxShadow {
    pub const NONE: Self =
        Self { offset: Vec2::ZERO, blur: 0.0, color: Color::TRANSPARENT };
}

impl Default for BoxShadow {
    fn default() -> Self {
        Self::NONE
    }
}

/// Which interaction state the user is currently in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TriggerSet {
    pub hover: bool,
    pub active: bool,
    pub focus: bool,
    pub disabled: bool,
}

impl TriggerSet {
    /// The most specific style that applies.
    ///
    /// Priority is disabled, then active, then hover, then focus.
    pub fn pick<'a>(&self, styles: &'a TriggerStyles) -> &'a Style {
        if self.disabled && let Some(s) = &styles.disabled {
            return s;
        }
        if self.active && let Some(s) = &styles.active {
            return s;
        }
        if self.hover && let Some(s) = &styles.hover {
            return s;
        }
        if self.focus && let Some(s) = &styles.focus {
            return s;
        }
        &styles.base
    }

    pub fn any(&self) -> bool {
        self.hover || self.active || self.focus || self.disabled
    }
}

/// Per-state style overrides, filled in by `on-hover:` and friends.
#[derive(Clone, Debug, Default)]
pub struct TriggerStyles {
    pub base: Style,
    pub hover: Option<Style>,
    pub active: Option<Style>,
    pub focus: Option<Style>,
    pub disabled: Option<Style>,
}

impl TriggerStyles {
    /// Applies a partial declaration block onto a state, inheriting from `base`.
    pub fn set_state(&mut self, which: State, partial: &Style, base: &Style) {
        let merged = merge_partial(base, partial);
        let slot = match which {
            State::Hover => &mut self.hover,
            State::Active => &mut self.active,
            State::Focus => &mut self.focus,
            State::Disabled => &mut self.disabled,
        };
        *slot = Some(match slot.take() {
            Some(prev) => merge_partial(&prev, &merged),
            None => merged,
        });
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Hover,
    Active,
    Focus,
    Disabled,
}

impl State {
    /// Maps a declaration-block key to a state.
    ///
    /// Accepts `on-hover`, `on_hover`, `:hover` and the bare word.
    pub fn from_property(prop: &str) -> Option<State> {
        Some(match prop {
            "hover" | "on-hover" | "on_hover" | ":hover" => State::Hover,
            "active" | "on-active" | "on_active" | ":active" | "pressed" => State::Active,
            "focus" | "on-focus" | "on_focus" | ":focus" => State::Focus,
            "disabled" | "on-disabled" | "on_disabled" => State::Disabled,
            _ => return None,
        })
    }
}

/// Every visual and layout property. `Default` is the identity, so
/// [`merge_partial`] can overlay a declaration block onto an inherited style.
#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    // --- layout ---
    pub display: Display,
    pub direction: Direction,
    pub wrap: WrapMode,
    pub gap: Dim,
    pub padding: Edges<Dim>,
    pub margin: Edges<Dim>,
    pub width: Dim,
    pub height: Dim,
    pub min_width: Option<Dim>,
    pub max_width: Option<Dim>,
    pub min_height: Option<Dim>,
    pub max_height: Option<Dim>,
    pub align_items: Align,
    pub justify_content: Justify,
    pub align_self: Option<Align>,
    pub justify_self: Option<Justify>,
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub flex_basis: Option<Dim>,
    pub position: Position,
    pub inset: Edges<Dim>,
    pub z_index: i32,
    pub overflow: Overflow,

    // --- visuals ---
    pub background: Option<Color>,
    pub background_image: Option<String>,
    pub background_fit: BgFit,
    pub color: Color,
    pub border_color: Color,
    pub border_width: f32,
    pub radius: Corners<f32>,
    pub shadow: BoxShadow,
    pub opacity: f32,
    pub clip: bool,
    pub filter: Filter,
    pub visible: bool,

    // --- text ---
    pub font_size: f32,
    pub font_family: String,
    pub font_weight: u16,
    pub font_style: FontStyle,
    pub line_height: f32,
    pub letter_spacing: f32,
    pub word_wrap: bool,
    pub text_align: TextAlign,

    // --- transform ---
    pub translate: Vec2,
    pub scale: Vec2,
    pub rotate: f32,

    // --- interaction / motion ---
    pub cursor: CursorKind,
    pub transition: Transition,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            display: Display::Flex,
            direction: Direction::Row,
            wrap: WrapMode::NoWrap,
            gap: Dim::ZERO,
            padding: Edges::default(),
            margin: Edges::default(),
            width: Dim::Auto,
            height: Dim::Auto,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            align_items: Align::Stretch,
            justify_content: Justify::Start,
            align_self: None,
            justify_self: None,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_basis: None,
            position: Position::Relative,
            inset: Edges::default(),
            z_index: 0,
            overflow: Overflow::Visible,

            background: None,
            background_image: None,
            background_fit: BgFit::Cover,
            color: Color::rgb(236, 240, 248, 1.0),
            border_color: Color::TRANSPARENT,
            border_width: 0.0,
            radius: Corners::splat(0.0),
            shadow: BoxShadow::NONE,
            opacity: 1.0,
            clip: false,
            filter: Filter::None,
            visible: true,

            font_size: 16.0,
            font_family: String::new(),
            font_weight: 400,
            font_style: FontStyle::Normal,
            line_height: 1.4,
            letter_spacing: 0.0,
            word_wrap: true,
            text_align: TextAlign::Left,

            translate: Vec2::ZERO,
            scale: Vec2::ONE,
            rotate: 0.0,

            cursor: CursorKind::Default,
            transition: Transition::default(),
        }
    }
}

/// `true` when nothing in `partial` overrides the identity default, i.e. it is
/// safe to overlay it without clobbering inherited values.
fn is_default_field<T: PartialEq + Default>(v: &T, d: &T) -> bool {
    v == d
}

/// Overlays the non-default fields of `partial` onto `base`.
///
/// This is what makes a declaration block *inherit*: `on_hover: "background: blue"`
/// only changes the background and leaves every other property alone.
pub fn merge_partial(base: &Style, partial: &Style) -> Style {
    let d = Style::default();
    let mut out = base.clone();

    macro_rules! layer {
        ($($f:ident),* $(,)?) => {
            $(
                if !is_default_field(&partial.$f, &d.$f) {
                    out.$f = partial.$f.clone();
                }
            )*
        };
    }

    layer!(
        display, direction, wrap, gap, padding, margin, width, height, min_width, max_width,
        min_height, max_height, align_items, justify_content, align_self, justify_self,
        flex_grow, flex_shrink, flex_basis, position, inset, z_index, overflow, background,
        background_image, background_fit, color, border_color, border_width, radius, shadow,
        opacity, clip, filter, visible, font_size, font_family, font_weight, font_style,
        line_height, letter_spacing, word_wrap, text_align, translate, scale, rotate, cursor,
        transition,
    );

    out
}

impl Style {
    /// Recognises `on-hover`, `hover`, `on_hover`, `:hover`, … as a state name.
    pub fn state_of(prop: &str) -> Option<State> {
        State::from_property(prop)
    }

    /// Builds a style from a CSS-like declaration block.
    pub fn parse(src: &str) -> Style {
        let mut s = Style::default();
        crate::css::apply_block(&mut s, src);
        s
    }

    /// The style a fresh element starts from, before declaration blocks.
    pub fn root() -> Style {
        let mut s = Style::default();
        s.direction = Direction::Column;
        s.align_items = Align::Start;
        s.width = Dim::Pct(100.0);
        s.height = Dim::Pct(100.0);
        s
    }

    pub fn width_px(mut self, v: f32) -> Self {
        self.width = Dim::Px(v);
        self
    }
    pub fn height_px(mut self, v: f32) -> Self {
        self.height = Dim::Px(v);
        self
    }
    pub fn pct_width(mut self, v: f32) -> Self {
        self.width = Dim::Pct(v);
        self
    }
    pub fn bg(mut self, c: Color) -> Self {
        self.background = Some(c);
        self
    }
    pub fn pad(mut self, v: f32) -> Self {
        self.padding = Edges::splat(Dim::Px(v));
        self
    }
    pub fn radius(mut self, v: f32) -> Self {
        self.radius = Corners::splat(v);
        self
    }
    pub fn color(mut self, c: Color) -> Self {
        self.color = c;
        self
    }
    pub fn font_size(mut self, v: f32) -> Self {
        self.font_size = v;
        self
    }
}