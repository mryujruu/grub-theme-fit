//! GRUB geometry values: `380`, `20%`, `50%-300`, `100%+4` (pixels, percent, or both).

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geom {
    pub pct: Option<f64>,
    pub px: f64,
}

impl Geom {
    pub fn parse(s: &str) -> Option<Geom> {
        let s = s.trim().trim_matches('"').trim();
        if s.is_empty() {
            return None;
        }
        if let Some(i) = s.find('%') {
            let pct: f64 = s[..i].trim().parse().ok()?;
            let rest = s[i + 1..].trim();
            if rest.is_empty() {
                return Some(Geom { pct: Some(pct), px: 0.0 });
            }
            let sign = match rest.chars().next()? {
                '+' => 1.0,
                '-' => -1.0,
                _ => return None,
            };
            let px: f64 = rest[1..].trim().parse().ok()?;
            Some(Geom { pct: Some(pct), px: sign * px })
        } else {
            let px: f64 = s.parse().ok()?;
            Some(Geom { pct: None, px })
        }
    }

    /// Size in pixels once the parent's size is known.
    pub fn resolve(&self, parent: f64) -> f64 {
        self.pct.map_or(0.0, |p| p * parent / 100.0) + self.px
    }
}

/// Format `p%`, `p%+x`, `p%-x` or plain pixels.
pub fn format_value(pct: Option<i64>, px: i64) -> String {
    match (pct, px) {
        (None, x) => x.to_string(),
        (Some(p), 0) => format!("{p}%"),
        (Some(p), x) if x > 0 => format!("{p}%+{x}"),
        (Some(p), x) => format!("{p}%{x}"),
    }
}

pub fn pct_text(p: f64) -> String {
    if p.fract() == 0.0 {
        format!("{}", p as i64)
    } else {
        format!("{p}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_three_forms() {
        assert_eq!(Geom::parse("380"), Some(Geom { pct: None, px: 380.0 }));
        assert_eq!(Geom::parse("20%"), Some(Geom { pct: Some(20.0), px: 0.0 }));
        assert_eq!(Geom::parse("100%-25"), Some(Geom { pct: Some(100.0), px: -25.0 }));
        assert_eq!(Geom::parse("50%+10"), Some(Geom { pct: Some(50.0), px: 10.0 }));
        assert_eq!(Geom::parse("\"60%\""), Some(Geom { pct: Some(60.0), px: 0.0 }));
        assert_eq!(Geom::parse("abc"), None);
        assert_eq!(Geom::parse("50%x"), None);
        assert_eq!(Geom::parse(""), None);
    }

    #[test]
    fn resolves_against_the_parent() {
        let g = Geom::parse("50%-300").unwrap();
        assert_eq!(g.resolve(1920.0), 660.0);
        assert_eq!(Geom::parse("380").unwrap().resolve(5000.0), 380.0);
    }

    #[test]
    fn formats_values() {
        assert_eq!(format_value(Some(20), -3), "20%-3");
        assert_eq!(format_value(Some(21), 3), "21%+3");
        assert_eq!(format_value(Some(60), 0), "60%");
        assert_eq!(format_value(None, 11), "11");
    }
}
