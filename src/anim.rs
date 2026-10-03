//! Declarative animations.
//!
//! There are two kinds, both driven by the engine with no handles to manage:
//!
//! * **Transitions** — style a property with `transition: "background 0.2s"` and
//!   any change to that property animates automatically.
//! * **Keyframe animations** — named recipes registered with [`anim!`] and
//!   triggered from a style with `animation: "pop 0.35s ease-out"`.
//!
//! ```
//! use mimui::anim;
//!
//! // Register once at startup, then reference it by name anywhere:
//! // `anim="card_in 0.4s ease-out"`.
//! anim!("card_in", [
//!     0.0 => { y: 16.0, opacity: 0.0 },
//!     1.0 => { y: 0.0, opacity: 1.0 },
//! ]);
//! ```

use crate::color::Color;
use crate::easing::{self, Easing};
use crate::geom::Vec2;
use crate::style::Style;
use std::collections::HashMap;
use std::sync::OnceLock;

/// Every property MiMUI knows how to animate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AnimProp {
    /// Horizontal offset added to the laid-out position.
    X,
    /// Vertical offset added to the laid-out position.
    Y,
    /// Extra width on top of the laid-out size.
    W,
    /// Extra height on top of the laid-out size.
    H,
    Opacity,
    Bg,
    BgAlpha,
    Fg,
    FgAlpha,
    /// All four corner radii at once.
    Radius,
    BorderWidth,
    ShadowBlur,
    /// Extra offset applied to this node's text.
    TextX,
    TextY,
    Scale,
    ScaleX,
    ScaleY,
    Rotate,
}

/// All animatable properties, in the order used by `transition: "all"`.
pub const ALL_PROPS: &[AnimProp] = &[
    AnimProp::X,
    AnimProp::Y,
    AnimProp::W,
    AnimProp::H,
    AnimProp::Opacity,
    AnimProp::Bg,
    AnimProp::BgAlpha,
    AnimProp::Fg,
    AnimProp::FgAlpha,
    AnimProp::Radius,
    AnimProp::BorderWidth,
    AnimProp::ShadowBlur,
    AnimProp::TextX,
    AnimProp::TextY,
    AnimProp::Scale,
    AnimProp::ScaleX,
    AnimProp::ScaleY,
    AnimProp::Rotate,
];

impl AnimProp {
    /// The CSS-ish name used in `transition:` and `anim!`.
    pub fn name(self) -> &'static str {
        match self {
            AnimProp::X => "x",
            AnimProp::Y => "y",
            AnimProp::W => "width",
            AnimProp::H => "height",
            AnimProp::Opacity => "opacity",
            AnimProp::Bg => "background",
            AnimProp::BgAlpha => "background-alpha",
            AnimProp::Fg => "color",
            AnimProp::FgAlpha => "color-alpha",
            AnimProp::Radius => "radius",
            AnimProp::BorderWidth => "border-width",
            AnimProp::ShadowBlur => "shadow-blur",
            AnimProp::TextX => "text-x",
            AnimProp::TextY => "text-y",
            AnimProp::Scale => "scale",
            AnimProp::ScaleX => "scale-x",
            AnimProp::ScaleY => "scale-y",
            AnimProp::Rotate => "rotate",
        }
    }

    /// Short aliases so `transition: "bg 0.2s"` works too.
    pub fn from_name(name: &str) -> Option<AnimProp> {
        let n = name.trim().to_ascii_lowercase();
        Some(match n.as_str() {
            "x" | "translate-x" | "left" => AnimProp::X,
            "y" | "translate-y" | "top" => AnimProp::Y,
            "w" | "width" => AnimProp::W,
            "h" | "height" => AnimProp::H,
            "opacity" | "alpha" => AnimProp::Opacity,
            "background" | "bg" | "background-color" => AnimProp::Bg,
            "background-alpha" | "bg-alpha" => AnimProp::BgAlpha,
            "color" | "fg" | "foreground" | "text-color" => AnimProp::Fg,
            "color-alpha" | "fg-alpha" | "text-alpha" => AnimProp::FgAlpha,
            "radius" | "border-radius" => AnimProp::Radius,
            "border-width" | "border" => AnimProp::BorderWidth,
            "shadow-blur" | "blur" => AnimProp::ShadowBlur,
            "text-x" => AnimProp::TextX,
            "text-y" => AnimProp::TextY,
            "scale" => AnimProp::Scale,
            "scale-x" => AnimProp::ScaleX,
            "scale-y" => AnimProp::ScaleY,
            "rotate" | "rotation" => AnimProp::Rotate,
            _ => return None,
        })
    }

    /// Reads the property's current value out of a style.
    pub fn read(self, s: &Style) -> AnimVal {
        match self {
            AnimProp::X => AnimVal::Num(0.0),
            AnimProp::Y => AnimVal::Num(0.0),
            AnimProp::W => AnimVal::Num(0.0),
            AnimProp::H => AnimVal::Num(0.0),
            AnimProp::Opacity => AnimVal::Num(s.opacity),
            AnimProp::Bg => AnimVal::Color(s.background.unwrap_or(Color::TRANSPARENT)),
            AnimProp::BgAlpha => AnimVal::Num(s.background.map_or(0.0, |c| c.a)),
            AnimProp::Fg => AnimVal::Color(s.color),
            AnimProp::FgAlpha => AnimVal::Num(s.color.a),
            AnimProp::Radius => AnimVal::Num(s.radius.max()),
            AnimProp::BorderWidth => AnimVal::Num(s.border_width),
            AnimProp::ShadowBlur => AnimVal::Num(s.shadow.blur),
            AnimProp::TextX => AnimVal::Num(0.0),
            AnimProp::TextY => AnimVal::Num(0.0),
            AnimProp::Scale => AnimVal::Num(s.scale.x),
            AnimProp::ScaleX => AnimVal::Num(s.scale.x),
            AnimProp::ScaleY => AnimVal::Num(s.scale.y),
            AnimProp::Rotate => AnimVal::Num(s.rotate.to_degrees()),
        }
    }

    /// Writes a value into a style.
    pub fn write(self, s: &mut Style, v: AnimVal) {
        match self {
            AnimProp::Opacity => s.opacity = v.num(1.0),
            AnimProp::Bg => {
                let c = v.color(Color::TRANSPARENT);
                s.background = Some(c);
            }
            AnimProp::BgAlpha => {
                let a = v.num(1.0);
                if let Some(c) = s.background {
                    s.background = Some(c.with_alpha(a));
                }
            }
            AnimProp::Fg => s.color = v.color(Color::WHITE),
            AnimProp::FgAlpha => s.color = s.color.with_alpha(v.num(1.0)),
            AnimProp::Radius => {
                let r = v.num(0.0).max(0.0);
                s.radius = crate::geom::Corners::splat(r);
            }
            AnimProp::BorderWidth => s.border_width = v.num(0.0).max(0.0),
            AnimProp::ShadowBlur => s.shadow.blur = v.num(0.0).max(0.0),
            AnimProp::Scale | AnimProp::ScaleX => {
                let n = v.num(1.0);
                if self == AnimProp::Scale {
                    s.scale = Vec2::splat(n);
                } else {
                    s.scale.x = n;
                }
            }
            AnimProp::ScaleY => s.scale.y = v.num(1.0),
            AnimProp::Rotate => s.rotate = f32::to_radians(v.num(0.0)),
            // X/Y/W/H/TextX/TextY are additive offsets applied at draw time.
            _ => {}
        }
    }
}

/// A single animated value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AnimVal {
    Num(f32),
    Color(Color),
    Xy(Vec2),
}

impl AnimVal {
    /// The scalar reading of this value (a color contributes its alpha).
    pub fn num(self, _default: f32) -> f32 {
        match self {
            AnimVal::Num(v) => v,
            AnimVal::Color(c) => c.a,
            AnimVal::Xy(v) => v.x,
        }
    }
    pub fn color(self, default: Color) -> Color {
        match self {
            AnimVal::Color(c) => c,
            AnimVal::Num(v) => default.with_alpha(default.a * v),
            AnimVal::Xy(v) => default.with_alpha(default.a * v.x),
        }
    }
    pub fn xy(self, default: Vec2) -> Vec2 {
        match self {
            AnimVal::Xy(v) => v,
            AnimVal::Num(v) => Vec2::splat(v),
            AnimVal::Color(c) => default * c.a,
        }
    }

    /// Brings this value into the same representation as `target` so that a
    /// type switch mid-animation does not panic.
    fn coerce(self, target: Self) -> Self {
        match (self, target) {
            (Self::Color(_), Self::Num(_)) => Self::Num(self.num(1.0)),
            (Self::Num(_), Self::Color(_)) => Self::Color(target.color(Color::TRANSPARENT)),
            (Self::Xy(_), Self::Num(_)) | (Self::Num(_), Self::Xy(_)) => {
                Self::Num(self.num(1.0))
            }
            (v, _) => v,
        }
    }

    pub fn lerp(self, o: Self, t: f32) -> Self {
        match (self, o) {
            (Self::Num(a), Self::Num(b)) => Self::Num(a + (b - a) * t),
            (Self::Color(a), Self::Color(b)) => Self::Color(a.lerp(b, t)),
            (Self::Xy(a), Self::Xy(b)) => Self::Xy(a.lerp(b, t)),
            (a, b) => a.coerce(b).lerp(b, t),
        }
    }
}

impl From<f32> for AnimVal {
    fn from(v: f32) -> Self {
        Self::Num(v)
    }
}
impl From<Color> for AnimVal {
    fn from(v: Color) -> Self {
        Self::Color(v)
    }
}
impl From<&str> for AnimVal {
    fn from(v: &str) -> Self {
        Self::Color(crate::color::parse_color(v).unwrap_or(Color::rgb(255, 0, 255, 1.0)))
    }
}
impl From<Vec2> for AnimVal {
    fn from(v: Vec2) -> Self {
        Self::Xy(v)
    }
}
impl From<(f32, f32)> for AnimVal {
    fn from(v: (f32, f32)) -> Self {
        Self::Xy(Vec2::new(v.0, v.1))
    }
}

/// Time-ordered tracks, one per property.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Keyframes {
    pub tracks: Vec<(AnimProp, Vec<(f32, AnimVal)>)>,
}

impl Keyframes {
    /// A const-constructible empty recipe.
    pub const fn new() -> Self {
        Keyframes { tracks: Vec::new() }
    }

    /// Samples the recipe at a normalized time, returning the properties that
    /// have a value there. Properties between two stops are interpolated.
    pub fn sample(&self, t: f32) -> Vec<(AnimProp, AnimVal)> {
        let mut out = Vec::with_capacity(self.tracks.len());
        for (prop, stops) in &self.tracks {
            if stops.is_empty() {
                continue;
            }
            out.push((*prop, sample_stops(stops, t)));
        }
        out
    }
}

fn sample_stops(stops: &[(f32, AnimVal)], t: f32) -> AnimVal {
    if stops.len() == 1 {
        return stops[0].1;
    }
    let first = stops[0];
    let last = stops[stops.len() - 1];
    if t <= first.0 {
        return first.1;
    }
    if t >= last.0 {
        return last.1;
    }
    for w in stops.windows(2) {
        let (t0, v0) = w[0];
        let (t1, v1) = w[1];
        if t >= t0 && t <= t1 {
            let span = t1 - t0;
            let k = if span <= f32::EPSILON { 0.0 } else { (t - t0) / span };
            return v0.coerce(v1).lerp(v1, k);
        }
    }
    last.1
}

/// Adds one timestamped group of property values.
pub fn push_kf(kfs: &mut Keyframes, at: f32, items: &[(AnimProp, AnimVal)]) {
    // Stops are written as fractions (`0.5`) or percentages (`50.0` / `50`).
    let t = if at > 1.0 { at / 100.0 } else { at };
    for (prop, val) in items {
        match kfs.tracks.iter_mut().find(|(p, _)| p == prop) {
            Some((_, stops)) => stops.push((t, *val)),
            None => kfs.tracks.push((*prop, vec![(t, *val)])),
        }
    }
    for (_, stops) in &mut kfs.tracks {
        stops.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        stops.dedup_by(|a, b| a.0 == b.0);
    }
}


/// How an animation repeats.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Iteration {
    Once,
    Count(u32),
    Infinite,
}

/// Short alias for [`Iteration`].
pub type Iter = Iteration;

impl Default for Iteration {
    fn default() -> Self {
        Iteration::Once
    }
}

/// Whether playback reverses on alternate cycles.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Direction {
    #[default]
    Normal,
    Alternate,
}

/// A parsed `animation:` shorthand.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimSpec {
    pub name: String,
    pub duration: f32,
    pub delay: f32,
    pub ease: Easing,
    pub iter: Iteration,
    pub direction: Direction,
}

impl Default for AnimSpec {
    fn default() -> Self {
        Self {
            name: String::new(),
            duration: 0.3,
            delay: 0.0,
            ease: Easing::default(),
            iter: Iteration::Once,
            direction: Direction::Normal,
        }
    }
}

/// Parses `"pop 0.35s ease-out 3 alternate"` (all parts after the name optional).
pub fn parse_anim_spec(s: &str) -> Option<AnimSpec> {
    let mut spec = AnimSpec::default();
    let mut name = None;
    let mut times = Vec::new();

    for tok in s.split_whitespace() {
        let t = tok.to_ascii_lowercase();
        if let Some(v) = parse_time(&t) {
            times.push(v);
            continue;
        }
        if t == "infinite" {
            spec.iter = Iteration::Infinite;
            continue;
        }
        if t == "alternate" || t == "reverse" || t == "alternate-reverse" {
            spec.direction = Direction::Alternate;
            continue;
        }
        if let Some(e) = easing::parse_easing(&t) {
            spec.ease = e;
            continue;
        }
        // A bare integer after the times is a repeat count.
        if let Ok(n) = t.parse::<u32>()
            && n > 0
            && spec.iter == Iteration::Once
        {
            spec.iter = Iteration::Count(n);
            continue;
        }
        if name.is_none() {
            name = Some(t);
        }
    }

    spec.name = name?;
    if !times.is_empty() {
        spec.duration = times[0].max(0.0);
    }
    if times.len() > 1 {
        spec.delay = times[1];
    }
    Some(spec)
}

/// Parses `150ms`, `0.35s`, `1.2` (already-seconds float) into seconds.
fn parse_time(t: &str) -> Option<f32> {
    if let Some(v) = t.strip_suffix("ms") {
        return v.parse::<f32>().ok().map(|n| n / 1000.0);
    }
    if let Some(v) = t.strip_suffix('s') {
        return v.parse::<f32>().ok();
    }
    None
}

/// `transition:` shorthand.
#[derive(Clone, Debug, PartialEq)]
pub struct Transition {
    pub duration: f32,
    pub delay: f32,
    pub ease: Easing,
    /// Empty means "every animatable property".
    pub props: Vec<AnimProp>,
}

impl Default for Transition {
    fn default() -> Self {
        Self { duration: 0.0, delay: 0.0, ease: Easing::default(), props: Vec::new() }
    }
}

impl Transition {
    /// Does this transition apply to `prop`?
    pub fn covers(&self, prop: AnimProp) -> bool {
        self.props.is_empty() || self.props.contains(&prop)
    }
    pub fn is_active(&self) -> bool {
        self.duration > 0.0
    }
}

/// Parses `"background 0.2s ease-out"`, `"0.2s"`, `"all 0.2s, scale 0.1s"`.
pub fn parse_transition(s: &str) -> Option<Transition> {
    let mut out = Transition::default();
    let mut any = false;

    for part in split_top(s, ',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let mut t = Transition::default();
        let mut saw_time = false;
        for tok in part.split_whitespace() {
            let tok = tok.to_ascii_lowercase();
            if let Some(v) = parse_time(&tok) {
                if saw_time {
                    t.delay = v;
                } else {
                    t.duration = v;
                    saw_time = true;
                }
                continue;
            }
            if let Some(e) = easing::parse_easing(&tok) {
                t.ease = e;
                continue;
            }
            if tok == "all" {
                t.props.clear();
                continue;
            }
            if let Some(p) = AnimProp::from_name(&tok) {
                t.props.push(p);
            }
        }
        if !saw_time {
            t.duration = 0.0;
        }
        out.duration = t.duration.max(out.duration);
        out.delay = t.delay;
        out.ease = t.ease;
        out.props.extend(t.props);
        any = any || saw_time;
    }

    if any || !out.props.is_empty() { Some(out) } else { None }
}

/// Splits on `sep` while respecting parentheses (`cubic-bezier(a,b,c,d)`).
fn split_top(s: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth -= 1;
                cur.push(c);
            }
            c if c == sep && depth == 0 => {
                out.push(std::mem::take(&mut cur));
            }
            c => cur.push(c),
        }
    }
    out.push(cur);
    out
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

type Registry = HashMap<String, &'static Keyframes>;

fn registry() -> &'static std::sync::RwLock<Registry> {
    static REG: OnceLock<std::sync::RwLock<Registry>> = OnceLock::new();
    REG.get_or_init(|| {
        let mut m = Registry::new();
        for (name, kfs) in builtins() {
            m.insert(name.to_string(), Box::leak(Box::new(kfs)));
        }
        std::sync::RwLock::new(m)
    })
}

/// Registers (or replaces) a named animation recipe.
pub fn register(name: &str, kfs: Keyframes) -> &'static str {
    registry()
        .write()
        .unwrap()
        .insert(name.to_ascii_lowercase(), Box::leak(Box::new(kfs)));
    Box::leak(name.to_string().into_boxed_str())
}

/// Looks up a recipe by name.
pub fn get(name: &str) -> Option<&'static Keyframes> {
    registry().read().unwrap().get(&name.to_ascii_lowercase()).copied()
}

/// Every registered name, for error messages and tooling.
pub fn names() -> Vec<String> {
    let guard = registry().read().unwrap();
    let mut v: Vec<String> = guard.keys().cloned().collect();
    v.sort();
    v
}

/// Looks up an [`AnimProp`] by name, panicking with the full list on a typo.
pub fn prop(name: &str) -> AnimProp {
    AnimProp::from_name(name).unwrap_or_else(|| {
        panic!(
            "MiMUI: unknown animatable property `{name}`. Valid names: {}",
            ALL_PROPS.iter().map(|p| p.name()).collect::<Vec<_>>().join(", ")
        )
    })
}



/// A keyframe animation running on one node.
#[derive(Clone, Debug)]
pub struct Animation {
    pub spec: AnimSpec,
    pub keyframes: &'static Keyframes,
    pub elapsed: f32,
}

impl Animation {
    pub fn new(spec: AnimSpec, keyframes: &'static Keyframes) -> Self {
        Self { spec, keyframes, elapsed: 0.0 }
    }

    /// Advances the clock. Returns true while it still needs frames.
    pub fn advance(&mut self, dt: f32) -> bool {
        self.elapsed += dt;
        match self.spec.iter {
            Iteration::Once => self.elapsed < self.spec.duration + self.spec.delay,
            Iteration::Count(n) => self.elapsed < self.spec.duration * n as f32 + self.spec.delay,
            Iteration::Infinite => true,
        }
    }

    /// The normalized, eased progress of the current cycle.
    pub fn progress(&self) -> f32 {
        let d = self.spec.duration;
        if d <= 0.0 {
            return 1.0;
        }
        let t = (self.elapsed - self.spec.delay) / d;
        if t < 0.0 {
            return 0.0;
        }
        let cycle = match self.spec.iter {
            Iteration::Once => 0.0,
            Iteration::Count(n) => (t / n as f32).floor(),
            Iteration::Infinite => t.floor(),
        };
        let cycles = match self.spec.iter {
            Iteration::Count(n) => n as f32,
            _ => f32::INFINITY,
        };
        let mut p = t - cycle;
        if cycles.is_finite() && p > 1.0 {
            p = 1.0;
        }
        if self.spec.direction == Direction::Alternate && cycle as i64 % 2 == 1 {
            p = 1.0 - p;
        }
        easing::apply(self.spec.ease, p)
    }

    /// Current values for this frame.
    pub fn sample(&self) -> Vec<(AnimProp, AnimVal)> {
        self.keyframes.sample(self.progress())
    }

    /// True once a finite animation has run to the end.
    pub fn is_finished(&self) -> bool {
        let elapsed = self.elapsed + self.spec.delay;
        match self.spec.iter {
            Iteration::Once => elapsed >= self.spec.duration,
            Iteration::Count(n) => elapsed >= self.spec.duration * n as f32,
            Iteration::Infinite => false,
        }
    }
}

/// The animations that ship with MiMUI, ready to use by name.
fn builtins() -> Vec<(&'static str, Keyframes)> {
    let n = AnimVal::Num;
    let kfs = |tracks: Vec<(AnimProp, Vec<(f32, AnimVal)>)>| Keyframes { tracks };

    vec![
        ("fade_in", kfs(vec![(AnimProp::Opacity, vec![(0.0, n(0.0)), (1.0, n(1.0))])])),
        (
            "fade_out",
            kfs(vec![(AnimProp::Opacity, vec![(0.0, n(1.0)), (1.0, n(0.0))])]),
        ),
        (
            "pop",
            kfs(vec![(
                AnimProp::Scale,
                vec![(0.0, n(0.86)), (0.6, n(1.04)), (1.0, n(1.0))],
            )]),
        ),
        (
            "zoom_in",
            kfs(vec![(
                AnimProp::Scale,
                vec![(0.0, n(0.5)), (1.0, n(1.0))],
            )]),
        ),
        (
            "zoom_out",
            kfs(vec![(AnimProp::Scale, vec![(0.0, n(1.0)), (1.0, n(0.5))])]),
        ),
        (
            "pulse",
            kfs(vec![(
                AnimProp::Scale,
                vec![(0.0, n(1.0)), (0.5, n(1.08)), (1.0, n(1.0))],
            )]),
        ),
        (
            "slide_up",
            kfs(vec![
                (AnimProp::Y, vec![(0.0, n(24.0)), (1.0, n(0.0))]),
                (AnimProp::Opacity, vec![(0.0, n(0.0)), (1.0, n(1.0))]),
            ]),
        ),
        (
            "slide_down",
            kfs(vec![
                (AnimProp::Y, vec![(0.0, n(-24.0)), (1.0, n(0.0))]),
                (AnimProp::Opacity, vec![(0.0, n(0.0)), (1.0, n(1.0))]),
            ]),
        ),
        (
            "slide_left",
            kfs(vec![
                (AnimProp::X, vec![(0.0, n(24.0)), (1.0, n(0.0))]),
                (AnimProp::Opacity, vec![(0.0, n(0.0)), (1.0, n(1.0))]),
            ]),
        ),
        (
            "slide_right",
            kfs(vec![
                (AnimProp::X, vec![(0.0, n(-24.0)), (1.0, n(0.0))]),
                (AnimProp::Opacity, vec![(0.0, n(0.0)), (1.0, n(1.0))]),
            ]),
        ),
        (
            "shake",
            kfs(vec![(
                AnimProp::X,
                vec![
                    (0.0, n(0.0)),
                    (0.2, n(-6.0)),
                    (0.4, n(6.0)),
                    (0.6, n(-4.0)),
                    (0.8, n(3.0)),
                    (1.0, n(0.0)),
                ],
            )]),
        ),
        (
            "float",
            kfs(vec![(
                AnimProp::Y,
                vec![(0.0, n(0.0)), (0.5, n(-8.0)), (1.0, n(0.0))],
            )]),
        ),
        (
            "spin",
            kfs(vec![(AnimProp::Rotate, vec![(0.0, n(0.0)), (1.0, n(360.0))])]),
        ),
        (
            "wobble",
            kfs(vec![(AnimProp::Rotate, vec![
                (0.0, n(0.0)),
                (0.25, n(-2.0)),
                (0.5, n(0.0)),
                (0.75, n(2.0)),
                (1.0, n(0.0)),
            ])]),
        ),
        (
            "bounce_in",
            kfs(vec![(
                AnimProp::Y,
                vec![(0.0, n(-40.0)), (0.55, n(6.0)), (0.8, n(-2.0)), (1.0, n(0.0))],
            )]),
        ),
        (
            "swing",
            kfs(vec![(AnimProp::Rotate, vec![
                (0.0, n(-12.0)),
                (0.5, n(6.0)),
                (1.0, n(0.0)),
            ])]),
        ),
        (
            "grow",
            kfs(vec![(AnimProp::BgAlpha, vec![(0.0, n(0.0)), (1.0, n(1.0))])]),
        ),
    ]
}

/// Helper used by the [`anim!`] macro to turn a property identifier into an
/// [`AnimProp`], and a value expression into an [`AnimVal`].
#[doc(hidden)]
pub fn item(p: AnimProp, v: AnimVal) -> (AnimProp, AnimVal) {
    (p, v)
}

/// Declares a named keyframe animation.
///
/// ```
/// use mimui::anim;
///
/// // Register at startup, then reference it by name from any style:
/// // `anim="drop 0.5s ease-out"`.
/// anim!("drop", [
///     0.0 => { y: -30.0, opacity: 0.0 },
///     0.7 => { opacity: 1.0 },
///     1.0 => { y: 0.0 },
/// ]);
/// ```
///
/// Positions are fractions of the duration: `0.0` is the start, `1.0` the end.
/// Values are numbers, colors (`"red"`, `Color::rgb(..)`), or `(x, y)` pairs.
#[macro_export]
macro_rules! anim {
    // Note the separator: a plain `,` between stops, plus one optional trailing
    // comma. Making the separator optional (`$(,)?`) would make the parser
    // ambiguous about which comma belongs where.
    ($name:expr, [$($at:expr => { $($p:ident: $v:expr),* $(,)? }),* $(,)?]) => {{
        let mut __kfs = $crate::anim::Keyframes::default();
        $(
            $crate::anim::push_kf(
                &mut __kfs,
                $at,
                &[$((
                    $crate::anim::prop(stringify!($p)),
                    $crate::anim::AnimVal::from($v),
                )),*],
            );
        )*
        $crate::anim::register($name, __kfs)
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_parsing() {
        let s = parse_anim_spec("pop 350ms ease-out 3 alternate").unwrap();
        assert_eq!(s.name, "pop");
        assert_eq!(s.duration, 0.35);
        assert_eq!(s.ease, Easing::EaseOut);
        assert_eq!(s.iter, Iteration::Count(3));
        assert_eq!(s.direction, Direction::Alternate);
    }

    #[test]
    fn transition_parsing() {
        let t = parse_transition("background 0.2s ease, scale 0.1s").unwrap();
        assert!(t.covers(AnimProp::Bg));
        assert!(t.covers(AnimProp::Scale));
        assert!(!t.covers(AnimProp::Y));
        assert_eq!(t.duration, 0.2);
    }

    #[test]
    fn keyframes_interpolate() {
        anim!("_test_kf", [0.0 => { x: 0.0 }, 1.0 => { x: 10.0 }]);
        let k = get("_test_kf").unwrap();
        let s = k.sample(0.5);
        assert_eq!(s[0].1, AnimVal::Num(5.0));
        assert_eq!(k.sample(2.0)[0].1, AnimVal::Num(10.0));
    }

    #[test]
    fn builtins_exist() {
        for n in ["fade_in", "pop", "shake", "spin", "slide_up"] {
            assert!(get(n).is_some(), "missing builtin {n}");
        }
    }

    #[test]
    fn animation_progress_wraps() {
        let spec = parse_anim_spec("fade_in 1s infinite").unwrap();
        let mut a = Animation::new(spec, get("fade_in").unwrap());
        a.elapsed = 1.5;
        assert!((a.progress() - 0.5).abs() < 1e-5);
    }
}
