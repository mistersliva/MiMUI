//! Colors and the very forgiving color parser used by the CSS-like syntax.

/// Straight (non-premultiplied) RGBA in the `0..=1` range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Default for Color {
    fn default() -> Self {
        Color::TRANSPARENT
    }
}

impl Color {
    pub const TRANSPARENT: Self = Self { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
    pub const BLACK: Self = Self { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const WHITE: Self = Self { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };

    /// Builds from 8-bit channels, `alpha` in `0..=1`.
    pub const fn rgb(r: u8, g: u8, b: u8, alpha: f32) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: alpha,
        }
    }

    /// Packs into `0xRRGGBBAA`.
    pub fn to_u32(self) -> u32 {
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
        (q(self.r) << 24) | (q(self.g) << 16) | (q(self.b) << 8) | q(self.a)
    }

    pub fn with_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    /// Multiplies the alpha channel by `f` — used to fade whole subtrees.
    pub fn fade(self, f: f32) -> Self {
        Self { a: self.a * f, ..self }
    }

    pub fn lerp(self, o: Self, t: f32) -> Self {
        Self {
            r: self.r + (o.r - self.r) * t,
            g: self.g + (o.g - self.g) * t,
            b: self.b + (o.b - self.b) * t,
            a: self.a + (o.a - self.a) * t,
        }
    }

    pub fn is_transparent(self) -> bool {
        self.a <= 0.0
    }

    /// `#rrggbbaa` for debug output.
    pub fn to_hex(self) -> String {
        format!("#{:08x}", self.to_u32())
    }
}

/// Parses a color. Accepts:
///
/// ```text
/// #rgb  #rgba  #rrggbb  #rrggbbaa
/// rgb(r, g, b)   rgba(r, g, b, a)   with r/g/b in 0..255 (or 0..1 with a dot)
/// transparent, white, black, red, …  (+ ~40 CSS keywords)
/// ```
pub fn parse_color(s: &str) -> Option<Color> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let lower = t.to_ascii_lowercase();

    if let Some(hex) = lower.strip_prefix('#') {
        return parse_hex(hex);
    }
    if let Some(inner) = lower.strip_prefix("rgba(").and_then(|r| r.strip_suffix(')')) {
        let parts = split_args(inner);
        if parts.len() == 4 {
            let rgb = (
                chan(parts[0], 255.0)?,
                chan(parts[1], 255.0)?,
                chan(parts[2], 255.0)?,
            );
            let a = if parts[3].ends_with('%') {
                parse_num(parts[3].trim_end_matches('%'))? / 100.0
            } else {
                chan(parts[3], 1.0)?
            };
            return Some(Color { r: rgb.0, g: rgb.1, b: rgb.2, a });
        }
        return None;
    }
    if let Some(inner) = lower.strip_prefix("rgb(").and_then(|r| r.strip_suffix(')')) {
        let parts = split_args(inner);
        if parts.len() == 3 {
            return Some(Color {
                r: chan(parts[0], 255.0)?,
                g: chan(parts[1], 255.0)?,
                b: chan(parts[2], 255.0)?,
                a: 1.0,
            });
        }
        return None;
    }

    named(&lower)
}

fn parse_hex(hex: &str) -> Option<Color> {
    let h = hex.trim();
    if !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let d = |c: u8| {
        let s = (c as char).to_digit(16).unwrap() as u8;
        (s * 16 + s) as f32 / 255.0
    };
    let n = |i: usize| h.as_bytes()[i];
    Some(match h.len() {
        3 | 4 => {
            let a = if h.len() == 4 { d(n(3)) } else { 1.0 };
            Color { r: d(n(0)), g: d(n(1)), b: d(n(2)), a }
        }
        6 | 8 => {
            let a = if h.len() == 8 {
                let (x, y) = (n(6), n(7));
                ((x as char).to_digit(16).unwrap() * 16 + (y as char).to_digit(16).unwrap()) as f32
                    / 255.0
            } else {
                1.0
            };
            let v = |i: usize| {
                let (x, y) = (n(i), n(i + 1));
                ((x as char).to_digit(16).unwrap() * 16 + (y as char).to_digit(16).unwrap()) as f32
                    / 255.0
            };
            Color { r: v(0), g: v(2), b: v(4), a }
        }
        _ => return None,
    })
}

fn split_args(s: &str) -> Vec<&str> {
    s.split(',').map(str::trim).filter(|p| !p.is_empty()).collect()
}

fn parse_num(s: &str) -> Option<f32> {
    s.trim().parse().ok()
}

/// One color channel: plain numbers are `0..255`, percentages are `0..100%`.
fn chan(s: &str, scale: f32) -> Option<f32> {
    if let Some(p) = s.strip_suffix('%') {
        Some(parse_num(p)? / 100.0)
    } else {
        let v = parse_num(s)?;
        // `0.5` on a 0..255 scale is clearly meant as a 0..1 float.
        if scale == 255.0 && v <= 1.0 { Some(v) } else { Some(v / scale) }
    }
}

fn named(n: &str) -> Option<Color> {
    let c = match n {
        "transparent" => Color::TRANSPARENT,
        "black" => Color::rgb(0, 0, 0, 1.0),
        "white" => Color::rgb(255, 255, 255, 1.0),
        "red" => Color::rgb(244, 67, 54, 1.0),
        "green" => Color::rgb(76, 175, 80, 1.0),
        "blue" => Color::rgb(33, 150, 243, 1.0),
        "yellow" => Color::rgb(255, 214, 0, 1.0),
        "orange" => Color::rgb(255, 145, 0, 1.0),
        "purple" => Color::rgb(156, 39, 176, 1.0),
        "magenta" | "fuchsia" => Color::rgb(233, 30, 99, 1.0),
        "cyan" | "aqua" => Color::rgb(0, 200, 220, 1.0),
        "pink" => Color::rgb(255, 128, 171, 1.0),
        "gray" | "grey" => Color::rgb(158, 158, 158, 1.0),
        "silver" => Color::rgb(192, 192, 192, 1.0),
        "darkgray" | "darkgrey" => Color::rgb(66, 66, 66, 1.0),
        "lightgray" | "lightgrey" => Color::rgb(214, 214, 214, 1.0),
        "navy" => Color::rgb(0, 0, 128, 1.0),
        "teal" => Color::rgb(0, 128, 128, 1.0),
        "olive" => Color::rgb(128, 128, 0, 1.0),
        "lime" => Color::rgb(0, 255, 0, 1.0),
        "brown" => Color::rgb(150, 75, 25, 1.0),
        "gold" => Color::rgb(255, 215, 0, 1.0),
        "indigo" => Color::rgb(48, 63, 159, 1.0),
        "violet" => Color::rgb(238, 130, 238, 1.0),
        _ => return None,
    };
    Some(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_usual_forms() {
        assert_eq!(parse_color("#fff").unwrap(), Color::rgb(255, 255, 255, 1.0));
        assert_eq!(parse_color("#ff0000").unwrap(), Color::rgb(255, 0, 0, 1.0));
        assert!(parse_color("#ff000080").unwrap().a > 0.49);
        assert!(parse_color("#ff000080").unwrap().a < 0.51);
        assert_eq!(parse_color("rgb(255,0,0)").unwrap(), Color::rgb(255, 0, 0, 1.0));
        assert_eq!(parse_color("red").unwrap(), parse_color("rgb(244, 67, 54)").unwrap());
        assert_eq!(parse_color("nonsense").map(|c| c.to_u32()), None);
    }

    #[test]
    fn lerp_and_hex_roundtrip() {
        let a = Color::rgb(0, 0, 0, 1.0);
        let b = Color::rgb(255, 255, 255, 1.0);
        assert_eq!(a.lerp(b, 0.5), Color { r: 0.5, g: 0.5, b: 0.5, a: 1.0 });
        assert_eq!(a.to_u32(), 0x0000_00ff);
        assert_eq!(b.to_u32(), 0xffff_ffff);
    }
}