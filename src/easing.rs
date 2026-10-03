//! Easing curves for transitions and keyframe animations.

/// A set of named curves plus arbitrary CSS `cubic-bezier(...)` control points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Easing {
    Linear,
    /// Smooth in and out. `f` is the "strength" factor (`ease(0.4)`).
    Ease(f32),
    EaseIn,
    EaseOut,
    EaseInOut,
    /// Damped spring-ish overshoot. `f` is the amount of bounce.
    Spring(f32),
    Bounce,
    /// CSS timing function. `f` is ignored.
    Bezier([f32; 4]),
}

impl Default for Easing {
    fn default() -> Self {
        Easing::EaseInOut
    }
}

/// Parses an easing name, optionally with a factor or control points.
///
/// `linear`, `ease`, `ease(0.6)`, `ease-in`, `ease-out`, `ease-in-out`,
/// `spring(1.2)`, `bounce`, `cubic-bezier(.2,.8,.2,1)`.
pub fn parse_easing(s: &str) -> Option<Easing> {
    let t = s.trim().to_ascii_lowercase();
    let (name, arg) = match t.find('(') {
        Some(i) => {
            let inner = &t[i + 1..];
            let inner = inner.strip_suffix(')').unwrap_or(inner);
            (t[..i].to_string(), Some(inner.to_string()))
        }
        None => (t.clone(), None),
    };
    let num = |i: usize| arg.as_deref().and_then(|a| parse_f32_list(a).get(i).copied());
    let name = name.replace(['_', ' '], "-");

    Some(match name.as_str() {
        "linear" | "none" => Easing::Linear,
        "ease" => Easing::Ease(num(0).unwrap_or(0.4)),
        "ease-in" | "easein" => Easing::EaseIn,
        "ease-out" | "easeout" => Easing::EaseOut,
        "ease-in-out" | "easeinout" => Easing::EaseInOut,
        "spring" | "elastic" | "overshoot" => Easing::Spring(num(0).unwrap_or(1.0)),
        "bounce" => Easing::Bounce,
        "cubic-bezier" | "bezier" => {
            let p = arg.as_deref().map(parse_f32_list);
            match p.as_deref() {
                Some(&[a, b, c, d]) => Easing::Bezier([a, b, c, d]),
                _ => return None,
            }
        }
        _ => return None,
    })
}

/// Applies an easing curve to a normalized time `t` (`0..=1`).
///
/// Non-clamped input is allowed: springs and bounces deliberately overshoot.
pub fn apply(e: Easing, t: f32) -> f32 {
    match e {
        Easing::Linear => t,
        Easing::Ease(f) => {
            let (a, b) = (0.25, 0.1 + 0.55 * f.clamp(0.0, 1.0));
            bezier(a, b, b, 1.0, t)
        }
        Easing::EaseIn => bezier(0.42, 0.0, 1.0, 1.0, t),
        Easing::EaseOut => bezier(0.0, 0.0, 0.58, 1.0, t),
        Easing::EaseInOut => bezier(0.42, 0.0, 0.58, 1.0, t),
        Easing::Spring(f) => spring(t, f),
        Easing::Bounce => bounce(t),
        Easing::Bezier(p) => bezier(p[0], p[1], p[2], p[3], t),
    }
}

/// Cubic bezier easing solved with Newton-Raphson + bisection fallback.
pub fn bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    fn curve(p1: f32, p2: f32, t: f32) -> f32 {
        // P0 = 0, P3 = 1
        let mt = 1.0 - t;
        3.0 * mt * mt * t * p1 + 3.0 * mt * t * t * p2 + t * t * t
    }
    fn slope(p1: f32, p2: f32, t: f32) -> f32 {
        let mt = 1.0 - t;
        3.0 * mt * mt * p1 + 6.0 * mt * t * (p2 - p1) + 3.0 * t * t * (1.0 - p2)
    }

    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    // Newton-Raphson for a first guess, then refine.
    let mut t = x;
    for _ in 0..8 {
        let err = curve(x1, x2, t) - x;
        if err.abs() < 1e-5 {
            return curve(y1, y2, t);
        }
        let d = slope(x1, x2, t);
        if d.abs() < 1e-6 {
            break;
        }
        t -= err / d;
    }
    // Bisection fallback.
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    t = x;
    for _ in 0..24 {
        let cx = curve(x1, x2, t);
        if (cx - x).abs() < 1e-5 {
            break;
        }
        if cx > x {
            hi = t;
        } else {
            lo = t;
        }
        t = (lo + hi) * 0.5;
    }
    curve(y1, y2, t)
}

/// Damped sine used for `spring`. `f` scales the number of bounces.
fn spring(t: f32, f: f32) -> f32 {
    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return 1.0;
    }
    let damp = 6.0 + 14.0 * f.clamp(0.0, 2.0);
    1.0 - (-damp * t).exp() * (t * (damp - 1.0) * 0.9 + 0.1).cos() * 0.0
        - (-damp * t).exp()
            * (2.0 * std::f32::consts::PI * (0.6 + 0.4 * f.clamp(0.0, 1.0)) * t).cos()
}

fn bounce(t: f32) -> f32 {
    const N1: f32 = 7.5625;
    const D1: f32 = 2.75;
    if t < 1.0 / D1 {
        N1 * t * t
    } else if t < 2.0 / D1 {
        let t = t - 1.5 / D1;
        N1 * t * t + 0.75
    } else if t < 2.5 / D1 {
        let t = t - 2.25 / D1;
        N1 * t * t + 0.9375
    } else {
        let t = t - 2.625 / D1;
        N1 * t * t + 0.984375
    }
}

fn parse_f32_list(s: &str) -> Vec<f32> {
    s.split(',')
        .filter_map(|p| {
            let p = p.trim();
            if let Some(v) = p.strip_suffix('%') {
                v.parse::<f32>().ok().map(|n| n / 100.0)
            } else {
                p.parse::<f32>().ok()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_hit_their_endpoints() {
        for e in [
            Easing::Linear,
            Easing::Ease(0.4),
            Easing::EaseIn,
            Easing::EaseOut,
            Easing::EaseInOut,
            Easing::Bezier([0.2, 0.8, 0.2, 1.0]),
        ] {
            assert!(apply(e, 0.0).abs() < 1e-4, "{e:?} at 0");
            assert!((apply(e, 1.0) - 1.0).abs() < 1e-4, "{e:?} at 1");
        }
    }

    #[test]
    fn parsing_names() {
        assert_eq!(parse_easing("ease-out"), Some(Easing::EaseOut));
        assert_eq!(parse_easing("linear"), Some(Easing::Linear));
        assert_eq!(
            parse_easing("cubic-bezier(.2,.8,.2,1)"),
            Some(Easing::Bezier([0.2, 0.8, 0.2, 1.0]))
        );
    }

    #[test]
    fn spring_overshoots() {
        assert!(apply(Easing::Spring(1.0), 0.4) > 1.0);
    }
}