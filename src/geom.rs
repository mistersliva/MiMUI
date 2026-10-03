//! Geometry primitives: points, rects, corner radii, edges and dimension values.

use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

/// A 2D point / size / offset.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };
    pub const ONE: Self = Self { x: 1.0, y: 1.0 };

    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
    pub const fn splat(v: f32) -> Self {
        Self { x: v, y: v }
    }
    pub fn x(&self) -> f32 {
        self.x
    }
    pub fn y(&self) -> f32 {
        self.y
    }
    /// Length of the vector.
    pub fn len(self) -> f32 {
        self.x.hypot(self.y)
    }
    pub fn is_zero(self) -> bool {
        self.x == 0.0 && self.y == 0.0
    }
    pub fn x_axis(self) -> Self {
        Self::new(self.x, 0.0)
    }
    pub fn y_axis(self) -> Self {
        Self::new(0.0, self.y)
    }
    pub fn max(self, o: Self) -> Self {
        Self::new(self.x.max(o.x), self.y.max(o.y))
    }
    pub fn min(self, o: Self) -> Self {
        Self::new(self.x.min(o.x), self.y.min(o.y))
    }
    /// Clamps each component to `lo ..= hi`.
    pub fn clamp(self, lo: f32, hi: f32) -> Self {
        Self::new(self.x.clamp(lo, hi), self.y.clamp(lo, hi))
    }
    /// Component-wise lerp.
    pub fn lerp(self, o: Self, t: f32) -> Self {
        self + (o - self) * t
    }
}

impl From<(f32, f32)> for Vec2 {
    fn from(v: (f32, f32)) -> Self {
        Self::new(v.0, v.1)
    }
}
impl From<[f32; 2]> for Vec2 {
    fn from(v: [f32; 2]) -> Self {
        Self::new(v[0], v[1])
    }
}
impl From<f32> for Vec2 {
    fn from(v: f32) -> Self {
        Self::splat(v)
    }
}
impl Add for Vec2 {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        Self::new(self.x + o.x, self.y + o.y)
    }
}
impl Sub for Vec2 {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        Self::new(self.x - o.x, self.y - o.y)
    }
}
impl Mul<f32> for Vec2 {
    type Output = Self;
    fn mul(self, s: f32) -> Self {
        Self::new(self.x * s, self.y * s)
    }
}
impl Mul<Vec2> for Vec2 {
    type Output = Self;
    fn mul(self, o: Self) -> Self {
        Self::new(self.x * o.x, self.y * o.y)
    }
}
impl Div<f32> for Vec2 {
    type Output = Self;
    fn div(self, s: f32) -> Self {
        Self::new(self.x / s, self.y / s)
    }
}
impl Neg for Vec2 {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y)
    }
}
impl AddAssign for Vec2 {
    fn add_assign(&mut self, o: Self) {
        *self = *self + o;
    }
}
impl SubAssign for Vec2 {
    fn sub_assign(&mut self, o: Self) {
        *self = *self - o;
    }
}

/// An axis-aligned rectangle.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0, w: 0.0, h: 0.0 };

    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    /// From an origin and an extent.
    pub fn from_origin_size(o: Vec2, s: Vec2) -> Self {
        Self { x: o.x, y: o.y, w: s.x, h: s.y }
    }
    pub fn size(&self) -> Vec2 {
        Vec2::new(self.w, self.h)
    }
    pub fn min(&self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }
    pub fn max(&self) -> Vec2 {
        Vec2::new(self.x + self.w, self.y + self.h)
    }
    pub fn center(&self) -> Vec2 {
        Vec2::new(self.x + self.w * 0.5, self.y + self.h * 0.5)
    }
    pub fn is_empty(&self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }
    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.x && p.x < self.x + self.w && p.y >= self.y && p.y < self.y + self.h
    }
    pub fn intersection(&self, o: &Rect) -> Rect {
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        let r = self.max().x.min(o.max().x);
        let b = self.max().y.min(o.max().y);
        Rect { x, y, w: (r - x).max(0.0), h: (b - y).max(0.0) }
    }
    pub fn union(&self, o: &Rect) -> Rect {
        let min = self.min().min(o.min());
        let max = self.max().max(o.max());
        Rect::from_origin_size(min, max - min)
    }
    /// Shrinks on all sides (negative grows).
    pub fn inset(&self, all: f32) -> Rect {
        self.inset_edges(Edges::splat(all))
    }
    pub fn inset_edges(&self, e: Edges<f32>) -> Rect {
        Rect {
            x: self.x + e.left,
            y: self.y + e.top,
            w: self.w - e.left - e.right,
            h: self.h - e.top - e.bottom,
        }
    }
    pub fn translate(&self, d: Vec2) -> Rect {
        Rect { x: self.x + d.x, y: self.y + d.y, ..*self }
    }
    /// Grows to include `other`.
    pub fn union_point(&mut self, p: Vec2) {
        let min = Vec2::new(self.x.min(p.x), self.y.min(p.y));
        let max = Vec2::new((self.x + self.w).max(p.x), (self.y + self.h).max(p.y));
        *self = Rect { x: min.x, y: min.y, w: max.x - min.x, h: max.y - min.y };
    }
}

/// Corner radii in `top-left, top-right, bottom-right, bottom-left` order.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Corners<T> {
    pub tl: T,
    pub tr: T,
    pub br: T,
    pub bl: T,
}

impl<T: Copy + Default> Corners<T> {
    pub fn splat(v: T) -> Self {
        Self { tl: v, tr: v, br: v, bl: v }
    }
    /// Builds from a 1–4 element CSS-ish shorthand.
    pub fn from_slice(v: &[T]) -> Self {
        match v.len() {
            0 => Self::default(),
            1 => Self::splat(v[0]),
            2 => Self { tl: v[0], tr: v[1], br: v[0], bl: v[1] },
            3 => Self { tl: v[0], tr: v[1], br: v[2], bl: v[1] },
            _ => Self { tl: v[0], tr: v[1], br: v[2], bl: v[3] },
        }
    }
}

impl Corners<f32> {
    /// Applies `f` to every corner.
    pub fn map(self, f: impl Fn(f32) -> f32) -> Corners<f32> {
        Corners { tl: f(self.tl), tr: f(self.tr), br: f(self.br), bl: f(self.bl) }
    }

    pub fn max(&self) -> f32 {
        self.tl.max(self.tr).max(self.br).max(self.bl)
    }
    /// Clamps radii so that opposite sides never overlap.
    pub fn clamp(&self, size: Vec2) -> Corners<f32> {
        let s = scale_factor(size.x, size.y, self);
        Corners {
            tl: self.tl * s.0,
            tr: self.tr * s.1,
            br: self.br * s.2,
            bl: self.bl * s.3,
        }
    }
}

/// Per-corner scale factors so that opposite radii never overlap.
///
/// Adjacent corners share an edge, so each is capped at half that edge.
fn scale_factor(w: f32, h: f32, c: &Corners<f32>) -> (f32, f32, f32, f32) {
    // Along the top and bottom edges the pair is (tl, tr) / (bl, br);
    // along the left and right edges it is (tl, bl) / (tr, br).
    let edge = |a: f32, b: f32, len: f32| {
        let sum = a + b;
        if sum <= 0.0 || len <= 0.0 {
            1.0
        } else {
            (len / 2.0 / sum.max(1e-6)).min(1.0)
        }
    };
    (
        edge(c.tl, c.tr, w).min(edge(c.tl, c.bl, h)),
        edge(c.tr, c.tl, w).min(edge(c.tr, c.br, h)),
        edge(c.br, c.tr, w).min(edge(c.br, c.bl, h)),
        edge(c.bl, c.tl, w).min(edge(c.bl, c.br, h)),
    )
}

/// Box edges in `top, right, bottom, left` order (CSS order).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Edges<T> {
    pub top: T,
    pub right: T,
    pub bottom: T,
    pub left: T,
}

impl<T: Copy + Default> Edges<T> {
    pub fn splat(v: T) -> Self {
        Self { top: v, right: v, bottom: v, left: v }
    }
    pub fn from_slice(v: &[T]) -> Self {
        match v.len() {
            0 => Self::default(),
            1 => Self::splat(v[0]),
            2 => Self { top: v[0], right: v[1], bottom: v[0], left: v[1] },
            3 => Self { top: v[0], right: v[1], bottom: v[2], left: v[1] },
            _ => Self { top: v[0], right: v[1], bottom: v[2], left: v[3] },
        }
    }
}

impl Edges<f32> {
    pub fn horizontal(&self) -> f32 {
        self.left + self.right
    }
    pub fn vertical(&self) -> f32 {
        self.top + self.bottom
    }
}

/// A length that can be absolute, relative to the parent, or content-sized.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dim {
    /// Fixed pixels.
    Px(f32),
    /// Percentage of the parent's size on the relevant axis.
    Pct(f32),
    /// Sized by content.
    Auto,
}

impl Default for Dim {
    fn default() -> Self {
        Dim::Auto
    }
}

impl Dim {
    pub const ZERO: Self = Dim::Px(0.0);

    pub fn is_auto(&self) -> bool {
        matches!(self, Dim::Auto)
    }
    pub fn is_definite(&self) -> bool {
        matches!(self, Dim::Px(_) | Dim::Pct(_))
    }
    /// Resolves against a parent size. `None` means "ask the content".
    pub fn resolve(&self, parent: f32) -> Option<f32> {
        match self {
            Dim::Px(v) => Some(*v),
            Dim::Pct(p) => Some(parent * p / 100.0),
            Dim::Auto => None,
        }
    }
    /// Resolves against a parent size, falling back to `fallback` for `Auto`.
    pub fn resolve_or(&self, parent: f32, fallback: f32) -> f32 {
        self.resolve(parent).unwrap_or(fallback)
    }
}

/// Rounds to whole device pixels to stop blurry edges.
pub fn snap(v: f32) -> f32 {
    v.round()
}

/// Clamp helper used by layout.
pub fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    if lo > hi { lo } else { v.clamp(lo, hi) }
}