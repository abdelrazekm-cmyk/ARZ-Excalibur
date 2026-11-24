//! Relative & absolute inline math evaluator for clip parameters
//! (Scale, Position, Rotation, Anchor Point, Opacity, Volume, Timecode offsets).

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ValueExpr {
    /// Set exact value, e.g. `100` or `-15` (when explicitly chosen as Set)
    Set(f64),
    /// Relative addition, e.g. `+25`
    Add(f64),
    /// Relative subtraction, e.g. `-25`
    Sub(f64),
    /// Relative multiplication, e.g. `*1.5`
    Mul(f64),
    /// Relative division, e.g. `/2`
    Div(f64),
    /// Percentage of current value, e.g. `%50`
    Percent(f64),
}

impl ValueExpr {
    /// Parses user submenu input like `"120"`, `"+20"`, `"-15.5"`, `"*2"`, `"/2"`, `"%75"`.
    pub fn parse(raw: &str) -> Option<Self> {
        let s = raw.trim().replace(',', ".");
        if s.is_empty() {
            return None;
        }
        if let Some(rest) = s.strip_prefix('+') {
            return rest.trim().parse::<f64>().ok().map(Self::Add);
        }
        if let Some(rest) = s.strip_prefix('*') {
            return rest.trim().parse::<f64>().ok().map(Self::Mul);
        }
        if let Some(rest) = s.strip_prefix('/') {
            let v = rest.trim().parse::<f64>().ok()?;
            return (v.abs() > f64::EPSILON).then_some(Self::Div(v));
        }
        if let Some(rest) = s.strip_prefix('%') {
            return rest.trim().parse::<f64>().ok().map(Self::Percent);
        }
        if let Some(rest) = s.strip_prefix('-') {
            return rest.trim().parse::<f64>().ok().map(Self::Sub);
        }
        s.parse::<f64>().ok().map(Self::Set)
    }

    /// Applies the expression to `current` parameter value.
    pub fn apply(self, current: f64) -> f64 {
        match self {
            Self::Set(v) => v,
            Self::Add(v) => current + v,
            Self::Sub(v) => current - v,
            Self::Mul(v) => current * v,
            Self::Div(v) => current / v,
            Self::Percent(v) => current * (v / 100.0),
        }
    }

    /// Returns true when the input forces relative calculation mode (`*`, `/`, `%`, `+`).
    pub fn is_relative_only(self) -> bool {
        matches!(self, Self::Add(_) | Self::Mul(_) | Self::Div(_) | Self::Percent(_))
    }
}
