//! Screen resolutions: parsing, aspect-ratio names, and the list of well-known sizes.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Res {
    pub w: u32,
    pub h: u32,
}

impl Res {
    pub fn new(w: u32, h: u32) -> Self {
        Res { w, h }
    }

    /// Accepts `1280x1024`, `1280x1024x32` (GRUB colour depth) and `1920x1080i`.
    pub fn parse(s: &str) -> Option<Res> {
        let s = s.trim().trim_matches('"');
        let mut parts = s.split(['x', 'X']);
        let w: u32 = parts.next()?.trim().parse().ok()?;
        let h_raw = parts.next()?.trim();
        let digits: String = h_raw.chars().take_while(|c| c.is_ascii_digit()).collect();
        let h: u32 = digits.parse().ok()?;
        if w == 0 || h == 0 || w > 32768 || h > 32768 {
            return None;
        }
        Some(Res { w, h })
    }

    pub fn aspect(&self) -> f64 {
        self.w as f64 / self.h as f64
    }

    pub fn area(&self) -> u64 {
        self.w as u64 * self.h as u64
    }

    /// `16:9`, `5:4`, ... or the reduced ratio when it is not a common one.
    pub fn ratio_label(&self) -> String {
        let a = self.aspect();
        let mut best: Option<(&str, f64)> = None;
        for (name, w, h) in RATIOS {
            let r = *w as f64 / *h as f64;
            let diff = (a - r).abs() / r;
            if diff <= 0.03 && best.map_or(true, |(_, d)| diff < d) {
                best = Some((name, diff));
            }
        }
        match best {
            Some((name, _)) => name.to_string(),
            None => {
                let g = gcd(self.w, self.h);
                format!("{}:{}", self.w / g, self.h / g)
            }
        }
    }

    /// True when both aspect ratios differ by more than `tolerance` (a fraction).
    pub fn aspect_differs(&self, other: &Res, tolerance: f64) -> bool {
        (self.aspect() - other.aspect()).abs() / other.aspect() > tolerance
    }
}

impl fmt::Display for Res {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}", self.w, self.h)
    }
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

const RATIOS: &[(&str, u32, u32)] = &[
    ("4:3", 4, 3),
    ("5:4", 5, 4),
    ("3:2", 3, 2),
    ("16:10", 16, 10),
    ("5:3", 5, 3),
    ("16:9", 16, 9),
    ("21:9", 21, 9),
    ("32:9", 32, 9),
];

/// Well-known sizes, grouped by aspect ratio.
pub const KNOWN: &[(&str, &[(u32, u32)])] = &[
    ("4:3", &[(640, 480), (800, 600), (1024, 768), (1152, 864), (1280, 960), (1400, 1050), (1600, 1200), (2048, 1536)]),
    ("5:4", &[(1280, 1024), (2560, 2048)]),
    ("3:2", &[(1920, 1280), (2160, 1440), (2256, 1504), (2736, 1824), (3000, 2000)]),
    ("16:10", &[(1280, 800), (1440, 900), (1680, 1050), (1920, 1200), (2560, 1600), (2880, 1800), (3840, 2400)]),
    ("16:9", &[(1280, 720), (1366, 768), (1600, 900), (1920, 1080), (2560, 1440), (3200, 1800), (3840, 2160), (5120, 2880), (7680, 4320)]),
    ("21:9", &[(2560, 1080), (3440, 1440), (3840, 1600), (5120, 2160)]),
    ("32:9", &[(3840, 1080), (5120, 1440), (7680, 2160)]),
];

pub fn all_known() -> Vec<Res> {
    KNOWN
        .iter()
        .flat_map(|(_, list)| list.iter().map(|&(w, h)| Res::new(w, h)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_grub_and_drm_forms() {
        assert_eq!(Res::parse("1280x1024"), Some(Res::new(1280, 1024)));
        assert_eq!(Res::parse("1280x1024x32"), Some(Res::new(1280, 1024)));
        assert_eq!(Res::parse("1920x1080i"), Some(Res::new(1920, 1080)));
        assert_eq!(Res::parse("\"800X600\""), Some(Res::new(800, 600)));
        assert_eq!(Res::parse("auto"), None);
        assert_eq!(Res::parse("0x600"), None);
        assert_eq!(Res::parse("1280"), None);
    }

    #[test]
    fn names_common_ratios() {
        assert_eq!(Res::new(1280, 1024).ratio_label(), "5:4");
        assert_eq!(Res::new(1920, 1080).ratio_label(), "16:9");
        assert_eq!(Res::new(1366, 768).ratio_label(), "16:9");
        assert_eq!(Res::new(1920, 1200).ratio_label(), "16:10");
        assert_eq!(Res::new(3440, 1440).ratio_label(), "21:9");
        assert_eq!(Res::new(1024, 768).ratio_label(), "4:3");
        assert_eq!(Res::new(1000, 300).ratio_label(), "10:3");
    }

    #[test]
    fn every_known_size_sits_in_its_group() {
        for (ratio, list) in KNOWN {
            for &(w, h) in *list {
                assert_eq!(&Res::new(w, h).ratio_label(), ratio, "{w}x{h}");
            }
        }
        assert!(all_known().len() >= 35);
    }
}
