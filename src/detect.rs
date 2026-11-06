//! Finding things on the system: the screen resolution, the theme file, and the size of its
//! background image. Every function takes its paths as arguments so tests can use temp folders.

use crate::resolutions::{all_known, Res};
use crate::theme::Theme;
use crate::value::Geom;
use std::fs;
use std::path::{Path, PathBuf};

pub const GRUB_DEFAULT: &str = "/etc/default/grub";
pub const DRM_DIR: &str = "/sys/class/drm";
pub const THEME_DIRS: &[&str] = &[
    "/boot/grub/themes",
    "/boot/grub2/themes",
    "/usr/share/grub/themes",
    "/usr/share/grub2/themes",
];

/// `KEY=value` from /etc/default/grub, without quotes. Comments and `export` are ignored.
pub fn grub_default_value(text: &str, key: &str) -> Option<String> {
    let mut found = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        if let Some(rest) = line.strip_prefix(key).and_then(|r| r.strip_prefix('=')) {
            let v = rest.trim().trim_matches(|c| c == '"' || c == '\'').to_string();
            found = Some(v); // the last assignment wins, like in the shell
        }
    }
    found
}

/// The resolution GRUB itself will use: the first `WxH` in GRUB_GFXMODE (`1280x1024,auto`).
pub fn gfxmode_resolution(grub_default_text: &str) -> Option<Res> {
    let v = grub_default_value(grub_default_text, "GRUB_GFXMODE")?;
    v.split(',').find_map(Res::parse)
}

/// Resolutions of connected displays, from the kernel's DRM folder: the first line of `modes`
/// is the display's preferred mode.
pub fn drm_resolutions(dir: &Path) -> Vec<(String, Res)> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else { return out };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for p in paths {
        let status = fs::read_to_string(p.join("status")).unwrap_or_default();
        if status.trim() != "connected" {
            continue;
        }
        let modes = fs::read_to_string(p.join("modes")).unwrap_or_default();
        if let Some(res) = modes.lines().next().and_then(Res::parse) {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            out.push((name, res));
        }
    }
    out
}

/// Where the screen size came from, so the user can check it.
pub struct Detected {
    pub res: Res,
    pub source: String,
}

pub fn detect_screen(grub_default: &Path, drm: &Path) -> Result<Detected, String> {
    if let Ok(text) = fs::read_to_string(grub_default) {
        if let Some(res) = gfxmode_resolution(&text) {
            return Ok(Detected { res, source: format!("GRUB_GFXMODE in {}", grub_default.display()) });
        }
    }
    let list = drm_resolutions(drm);
    if let Some((name, res)) = list.first() {
        let more = if list.len() > 1 { format!(" (first of {} connected displays)", list.len()) } else { String::new() };
        return Ok(Detected { res: *res, source: format!("the connected display {name}{more}") });
    }
    Err("could not detect the screen resolution (no WxH in GRUB_GFXMODE, and no connected display in /sys/class/drm). \
Use `manual` to choose one."
        .to_string())
}

/// Resolve a theme argument: a theme.txt, a theme folder, a theme name, or (no argument) GRUB_THEME.
pub fn find_theme(arg: Option<&str>, grub_default: &Path, search: &[&str]) -> Result<PathBuf, String> {
    match arg {
        Some(a) => {
            let p = PathBuf::from(a);
            if p.is_file() {
                return Ok(p);
            }
            if p.is_dir() && p.join("theme.txt").is_file() {
                return Ok(p.join("theme.txt"));
            }
            if !a.contains('/') {
                for dir in search {
                    let c = Path::new(dir).join(a).join("theme.txt");
                    if c.is_file() {
                        return Ok(c);
                    }
                }
            }
            Err(format!("no theme found for `{a}`. Give a theme.txt, a theme folder, or the name of a theme in {}", search.join(", ")))
        }
        None => {
            let text = fs::read_to_string(grub_default)
                .map_err(|_| format!("no theme given, and {} could not be read to find GRUB_THEME", grub_default.display()))?;
            let v = grub_default_value(&text, "GRUB_THEME")
                .ok_or_else(|| format!("no theme given, and {} has no GRUB_THEME", grub_default.display()))?;
            let p = PathBuf::from(&v);
            if p.is_file() {
                Ok(p)
            } else {
                Err(format!("GRUB_THEME points to {v}, which does not exist"))
            }
        }
    }
}

/// Width and height of a PNG or JPEG file.
pub fn image_size(path: &Path) -> Option<(u32, u32)> {
    let b = fs::read(path).ok()?;
    if b.len() > 24 && b[..8] == [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] {
        let w = u32::from_be_bytes([b[16], b[17], b[18], b[19]]);
        let h = u32::from_be_bytes([b[20], b[21], b[22], b[23]]);
        return Some((w, h));
    }
    if b.len() > 4 && b[0] == 0xFF && b[1] == 0xD8 {
        let mut i = 2;
        while i + 9 < b.len() {
            if b[i] != 0xFF {
                i += 1;
                continue;
            }
            let marker = b[i + 1];
            if marker == 0xD8 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) || marker == 0xFF {
                i += if marker == 0xFF { 1 } else { 2 };
                continue;
            }
            let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
            let is_sof = (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
            if is_sof {
                let h = u16::from_be_bytes([b[i + 5], b[i + 6]]) as u32;
                let w = u16::from_be_bytes([b[i + 7], b[i + 8]]) as u32;
                return Some((w, h));
            }
            i += 2 + len;
        }
    }
    None
}

#[derive(Debug, PartialEq)]
pub enum Confidence {
    Strong,
    Weak,
}

pub struct Guess {
    pub res: Res,
    pub confidence: Confidence,
    pub reason: String,
}

/// The right and bottom edge, in pixels, of everything the theme places with plain pixel values.
pub fn pixel_extent(theme: &Theme) -> (f64, f64) {
    let (mut right, mut bottom) = (0.0f64, 0.0f64);
    for b in &theme.blocks {
        let get = |name: &str| b.prop(name).and_then(|p| Geom::parse(&p.raw)).filter(|g| g.pct.is_none()).map(|g| g.px);
        if let (Some(l), Some(w)) = (get("left"), get("width")) {
            right = right.max(l + w);
        }
        if let (Some(t), Some(h)) = (get("top"), get("height")) {
            bottom = bottom.max(t + h);
        }
    }
    (right, bottom)
}

pub fn has_pixel_values(theme: &Theme) -> bool {
    fn walk(blocks: &[crate::theme::Block]) -> bool {
        blocks.iter().any(|b| {
            ["left", "top", "width", "height"]
                .iter()
                .filter_map(|n| b.prop(n))
                .filter_map(|p| Geom::parse(&p.raw))
                .any(|g| g.pct.is_none())
                || walk(&b.children)
        })
    }
    walk(&theme.blocks)
        || theme.globals.iter().any(|p| {
            ["terminal-left", "terminal-top", "terminal-width", "terminal-height"].contains(&p.name.as_str())
                && Geom::parse(&p.raw).is_some_and(|g| g.pct.is_none())
        })
}

/// Guess the resolution the theme was designed for. The background image shows the aspect ratio
/// the author used, and the layout shows how big the screen had to be.
pub fn guess_design(theme: &Theme, theme_dir: &Path) -> Option<Guess> {
    let (right, bottom) = pixel_extent(theme);
    let bg = theme
        .global("desktop-image")
        .and_then(|p| image_size(&theme_dir.join(p.value())))
        .map(|(w, h)| Res::new(w, h));
    let fits = |r: &Res| r.w as f64 >= right && r.h as f64 >= bottom;

    if let Some(bg) = bg {
        let mut same: Vec<Res> = all_known().into_iter().filter(|r| !r.aspect_differs(&bg, 0.02) && fits(r)).collect();
        same.sort_by_key(|r| r.area());
        if let Some(r) = same.first() {
            return Some(Guess {
                res: *r,
                confidence: Confidence::Strong,
                reason: format!(
                    "the background is {}x{} ({}), and the layout reaches {}x{}, so {} is the smallest {} size it fits",
                    bg.w, bg.h, bg.ratio_label(), right as u32, bottom as u32, r, r.ratio_label()
                ),
            });
        }
        return Some(Guess {
            res: bg,
            confidence: Confidence::Weak,
            reason: format!("the size of the background image ({}x{}); no known size fits the layout", bg.w, bg.h),
        });
    }
    if right > 0.0 && bottom > 0.0 {
        let mut all: Vec<Res> = all_known().into_iter().filter(fits).collect();
        all.sort_by_key(|r| r.area());
        if let Some(r) = all.first() {
            return Some(Guess {
                res: *r,
                confidence: Confidence::Weak,
                reason: format!("the smallest known size that fits a layout reaching {}x{} (no background image to confirm it)", right as u32, bottom as u32),
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("gtf-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn reads_values_from_etc_default_grub() {
        let t = "# GRUB_GFXMODE=800x600\nGRUB_GFXMODE=\"1280x1024x32,auto\"\nexport GRUB_THEME='/boot/grub/themes/x/theme.txt'\n";
        assert_eq!(gfxmode_resolution(t), Some(Res::new(1280, 1024)));
        assert_eq!(grub_default_value(t, "GRUB_THEME").unwrap(), "/boot/grub/themes/x/theme.txt");
        assert_eq!(gfxmode_resolution("GRUB_GFXMODE=auto\n"), None);
        assert_eq!(gfxmode_resolution("GRUB_GFXMODE=auto,1920x1080\n"), Some(Res::new(1920, 1080)));
        assert_eq!(gfxmode_resolution(""), None);
    }

    #[test]
    fn reads_connected_displays_from_drm() {
        let d = tmp("drm");
        for (name, status, modes) in [("card0-HDMI-A-1", "connected", "1280x1024\n1024x768\n"), ("card0-DP-1", "disconnected", "1920x1080\n")] {
            let c = d.join(name);
            fs::create_dir_all(&c).unwrap();
            fs::write(c.join("status"), status).unwrap();
            fs::write(c.join("modes"), modes).unwrap();
        }
        let got = drm_resolutions(&d);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].1, Res::new(1280, 1024));
    }

    #[test]
    fn gfxmode_wins_over_the_display_and_the_source_is_named() {
        let d = tmp("auto");
        let g = d.join("grub");
        fs::write(&g, "GRUB_GFXMODE=1024x768\n").unwrap();
        let got = detect_screen(&g, &d.join("none")).unwrap();
        assert_eq!(got.res, Res::new(1024, 768));
        assert!(got.source.contains("GRUB_GFXMODE"));
        assert!(detect_screen(&d.join("missing"), &d.join("none")).is_err());
    }

    #[test]
    fn finds_a_theme_by_file_folder_name_or_grub_theme() {
        let d = tmp("find");
        let theme = d.join("themes").join("nexus");
        fs::create_dir_all(&theme).unwrap();
        fs::write(theme.join("theme.txt"), "a: b").unwrap();
        let none = d.join("nogrub");
        assert!(find_theme(Some(theme.to_str().unwrap()), &none, &[]).is_ok());
        assert!(find_theme(Some(theme.join("theme.txt").to_str().unwrap()), &none, &[]).is_ok());
        assert!(find_theme(Some("nexus"), &none, &[d.join("themes").to_str().unwrap()]).is_ok());
        assert!(find_theme(Some("nexus"), &none, &[]).is_err());
        let g = d.join("grub");
        fs::write(&g, format!("GRUB_THEME=\"{}\"\n", theme.join("theme.txt").display())).unwrap();
        assert_eq!(find_theme(None, &g, &[]).unwrap(), theme.join("theme.txt"));
    }

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut v = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, b'I', b'H', b'D', b'R'];
        v.extend(w.to_be_bytes());
        v.extend(h.to_be_bytes());
        v.extend([8, 2, 0, 0, 0]);
        v
    }

    #[test]
    fn reads_png_and_jpeg_sizes() {
        let d = tmp("img");
        fs::write(d.join("a.png"), png(3840, 2160)).unwrap();
        assert_eq!(image_size(&d.join("a.png")), Some((3840, 2160)));
        // SOI, APP0 (len 4), SOF0 with 1080x1920
        let jpg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0, 0, 0xFF, 0xC0, 0x00, 0x11, 8, 0x04, 0x38, 0x07, 0x80, 3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1];
        let mut f = fs::File::create(d.join("b.jpg")).unwrap();
        f.write_all(&jpg).unwrap();
        assert_eq!(image_size(&d.join("b.jpg")), Some((1920, 1080)));
        assert_eq!(image_size(&d.join("missing.png")), None);
    }

    #[test]
    fn design_resolution_comes_from_the_background_ratio_and_the_layout() {
        let d = tmp("design");
        fs::write(d.join("background.png"), png(3840, 2160)).unwrap();
        let t = Theme::parse(crate::theme::tests_sample::NEXUS).unwrap();
        let g = guess_design(&t, &d).unwrap();
        assert_eq!(g.res, Res::new(1920, 1080));
        assert_eq!(g.confidence, Confidence::Strong);
        // without a background it is only a weak guess
        let g = guess_design(&t, &d.join("empty")).unwrap();
        assert_eq!(g.confidence, Confidence::Weak);
    }

    #[test]
    fn a_theme_without_pixel_values_has_nothing_to_convert() {
        let t = Theme::parse("+ boot_menu { left = 10% top = 20% width = 80% height = 50% }").unwrap();
        assert!(!has_pixel_values(&t));
        assert!(has_pixel_values(&Theme::parse(crate::theme::tests_sample::NEXUS).unwrap()));
    }
}
