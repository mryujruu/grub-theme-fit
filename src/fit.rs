//! The conversion: turn a theme designed for one resolution into a layout for another.
//!
//! Only the values that need to change are rewritten. Everything else (comments, spacing, colours,
//! fonts, image names) is copied from the original text untouched.

use crate::resolutions::Res;
use crate::theme::{Block, Theme};
use crate::value::{format_value, pct_text, Geom};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Style {
    /// `20%-3`: percent plus a few pixels, exact at the chosen resolution.
    Exact,
    /// `20%`: whole percentages only; keeps working at any resolution, a little less precise.
    Percent,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Layout {
    /// Positions and sizes follow each axis (a 16:9 layout is stretched onto a 5:4 screen).
    Stretch,
    /// The whole layout keeps its shape, is scaled by one factor and centred.
    Center,
}

#[derive(Clone, Debug)]
pub struct Plan {
    pub design: Res,
    pub target: Res,
    pub style: Style,
    pub layout: Layout,
    pub scale_items: bool,
    pub crop_background: bool,
}

#[derive(Debug, Clone)]
pub struct Change {
    pub name: String,
    pub line: usize,
    pub old: String,
    pub new: String,
}

#[derive(Debug)]
pub struct Fitted {
    pub text: String,
    pub changes: Vec<Change>,
    pub warnings: Vec<String>,
}

struct Edit {
    start: usize,
    end: usize,
    text: String,
}

/// Size of a container in pixels, in the design and at the target resolution.
#[derive(Clone, Copy)]
struct Cont {
    dw: f64,
    dh: f64,
    tw: f64,
    th: f64,
}

#[derive(Clone, Copy, PartialEq)]
enum Axis {
    X,
    Y,
}

#[derive(Clone, Copy, PartialEq)]
enum Role {
    Pos,
    Size,
}

const ITEM_PROPS: &[&str] = &[
    "item_height", "item_padding", "item_spacing", "item_icon_space", "icon_width", "icon_height",
    "scrollbar_width", "scrollbar_left_pad", "scrollbar_right_pad", "scrollbar_top_pad", "scrollbar_bottom_pad",
];

fn px_min(v: f64, role: Role) -> i64 {
    let r = v.round() as i64;
    if role == Role::Size && r < 1 {
        1
    } else {
        r
    }
}

fn mixed(pct: f64, px: i64) -> String {
    match px {
        0 => format!("{}%", pct_text(pct)),
        x if x > 0 => format!("{}%+{x}", pct_text(pct)),
        x => format!("{}%{x}", pct_text(pct)),
    }
}

fn emit(v_t: f64, parent_t: f64, style: Style, role: Role) -> String {
    let p = (v_t / parent_t * 100.0).round() as i64;
    match style {
        Style::Exact => {
            if p == 0 {
                return format_value(None, px_min(v_t, role));
            }
            let x = (v_t - p as f64 * parent_t / 100.0).round() as i64;
            format_value(Some(p), x)
        }
        Style::Percent => {
            if p >= 2 || (role == Role::Pos && p >= 1) {
                format_value(Some(p), 0)
            } else {
                format_value(None, px_min(v_t, role))
            }
        }
    }
}

/// The new text for one geometry value, or None when it should stay as it is.
fn fit_value(g: Geom, axis: Axis, role: Role, c: &Cont, plan: &Plan, root: bool) -> Option<String> {
    let (pd, pt) = if axis == Axis::X { (c.dw, c.tw) } else { (c.dh, c.th) };
    let ratio = pt / pd;
    if plan.layout == Layout::Center && root {
        let s = (c.tw / c.dw).min(c.th / c.dh);
        let off = (pt - s * pd) / 2.0;
        let v = g.resolve(pd);
        let v_t = if role == Role::Pos { off + s * v } else { s * v };
        return Some(emit(v_t, pt, plan.style, role));
    }
    match g.pct {
        Some(_) if g.px == 0.0 => None, // already relative to the screen
        Some(p) => Some(mixed(p, (g.px * ratio).round() as i64)),
        None => Some(emit(g.px * ratio, pt, plan.style, role)),
    }
}

struct Ctx<'a> {
    theme: &'a Theme,
    plan: &'a Plan,
    edits: Vec<Edit>,
    changes: Vec<Change>,
    warnings: Vec<String>,
    fonts: Vec<String>,
}

impl<'a> Ctx<'a> {
    fn record(&mut self, name: String, prop: &crate::theme::Prop, new: String) {
        let text = if prop.quoted() { format!("\"{new}\"") } else { new.clone() };
        if new == prop.value() {
            return;
        }
        self.changes.push(Change { name, line: self.theme.line_of(prop.start), old: prop.value().to_string(), new });
        self.edits.push(Edit { start: prop.start, end: prop.end, text });
    }

    fn geometry(&mut self, name: &str, prop: &crate::theme::Prop, axis: Axis, role: Role, c: &Cont, root: bool) -> Option<Geom> {
        let Some(g) = Geom::parse(&prop.raw) else {
            self.warnings.push(format!("{name}: could not read the value `{}`, so it was left as it is", prop.raw));
            return None;
        };
        let new = fit_value(g, axis, role, c, self.plan, root);
        match new {
            Some(n) => {
                let parsed = Geom::parse(&n);
                self.record(name.to_string(), prop, n);
                parsed.or(Some(g))
            }
            None => Some(g),
        }
    }

    fn walk(&mut self, blocks: &[Block], cont: Option<Cont>, root: bool) {
        for (i, b) in blocks.iter().enumerate() {
            let same = blocks.iter().filter(|x| x.kind == b.kind).count();
            let label = match b.id() {
                Some(id) => format!("{}#{id}", b.kind),
                None if same > 1 => format!("{}[{}]", b.kind, i + 1),
                None => b.kind.clone(),
            };
            for p in &b.props {
                if p.name.contains("font") {
                    let f = p.value().to_string();
                    if !f.is_empty() && !self.fonts.contains(&f) {
                        self.fonts.push(f);
                    }
                }
            }

            let (mut new_w, mut new_h) = (None, None);
            let mut orig_w = None;
            let mut orig_h = None;
            match cont {
                Some(c) => {
                    for p in &b.props {
                        let (axis, role) = match p.name.as_str() {
                            "left" => (Axis::X, Role::Pos),
                            "top" => (Axis::Y, Role::Pos),
                            "width" => (Axis::X, Role::Size),
                            "height" => (Axis::Y, Role::Size),
                            _ => continue,
                        };
                        let g = self.geometry(&format!("{label}.{}", p.name), p, axis, role, &c, root);
                        match p.name.as_str() {
                            "width" => {
                                new_w = g;
                                orig_w = Geom::parse(&p.raw);
                            }
                            "height" => {
                                new_h = g;
                                orig_h = Geom::parse(&p.raw);
                            }
                            _ => {}
                        }
                    }
                    if self.plan.scale_items && b.kind == "boot_menu" {
                        let s = (c.tw / c.dw).min(c.th / c.dh);
                        for p in &b.props {
                            if !ITEM_PROPS.contains(&p.name.as_str()) {
                                continue;
                            }
                            if let Some(g) = Geom::parse(&p.raw).filter(|g| g.pct.is_none()) {
                                let v = ((g.px * s).round() as i64).max(1);
                                self.record(format!("{label}.{}", p.name), p, v.to_string());
                            }
                        }
                    }
                }
                None => {
                    let has_px = ["left", "top", "width", "height"]
                        .iter()
                        .filter_map(|n| b.prop(n))
                        .filter_map(|p| Geom::parse(&p.raw))
                        .any(|g| g.pct.is_none());
                    if has_px {
                        self.warnings.push(format!(
                            "{label}: its pixel values sit inside a container whose size is not given (no width and height), so they were left as they are"
                        ));
                    }
                }
            }

            if !b.children.is_empty() {
                let child_cont = match (cont, orig_w, orig_h, new_w, new_h) {
                    (Some(c), Some(ow), Some(oh), Some(nw), Some(nh)) => Some(Cont {
                        dw: ow.resolve(c.dw),
                        dh: oh.resolve(c.dh),
                        tw: nw.resolve(c.tw),
                        th: nh.resolve(c.th),
                    }),
                    _ => None,
                };
                self.walk(&b.children, child_cont, false);
            }
        }
    }
}

pub fn fit(theme: &Theme, plan: &Plan) -> Fitted {
    let screen = Cont { dw: plan.design.w as f64, dh: plan.design.h as f64, tw: plan.target.w as f64, th: plan.target.h as f64 };
    let mut ctx = Ctx { theme, plan, edits: Vec::new(), changes: Vec::new(), warnings: Vec::new(), fonts: Vec::new() };

    // global terminal box
    for p in &theme.globals {
        if p.name.contains("font") {
            let f = p.value().to_string();
            if !f.is_empty() && !ctx.fonts.contains(&f) {
                ctx.fonts.push(f);
            }
        }
        let (axis, role) = match p.name.as_str() {
            "terminal-left" => (Axis::X, Role::Pos),
            "terminal-top" => (Axis::Y, Role::Pos),
            "terminal-width" => (Axis::X, Role::Size),
            "terminal-height" => (Axis::Y, Role::Size),
            _ => continue,
        };
        let name = p.name.clone();
        ctx.geometry(&name, p, axis, role, &screen, true);
    }
    ctx.walk(&theme.blocks, Some(screen), true);

    if plan.crop_background {
        match theme.global("desktop-image-scale-method") {
            Some(p) => {
                if p.value() != "crop" {
                    ctx.changes.push(Change { name: p.name.clone(), line: theme.line_of(p.start), old: p.value().to_string(), new: "crop".to_string() });
                    ctx.edits.push(Edit { start: p.start, end: p.end, text: "\"crop\"".to_string() });
                }
            }
            None => match theme.global("desktop-image") {
                Some(img) => {
                    let eol = theme.src[img.end..].find('\n').map(|n| img.end + n).unwrap_or(theme.src.len());
                    ctx.changes.push(Change { name: "desktop-image-scale-method".into(), line: theme.line_of(img.start) + 1, old: "(not set)".into(), new: "crop".into() });
                    ctx.edits.push(Edit { start: eol, end: eol, text: "\ndesktop-image-scale-method: \"crop\"".to_string() });
                }
                None => ctx.warnings.push("--crop-background was asked for, but the theme has no desktop-image".to_string()),
            },
        }
    }

    let mut warnings = std::mem::take(&mut ctx.warnings);
    let (sx, sy) = (screen.tw / screen.dw, screen.th / screen.dh);
    if plan.design.aspect_differs(&plan.target, 0.02) {
        let how = match plan.layout {
            Layout::Stretch => "The layout is stretched to the new shape. Use `--layout center` to keep its proportions instead.",
            Layout::Center => "The layout keeps its proportions and is centred, so there are empty bands at the sides or top and bottom.",
        };
        warnings.push(format!(
            "The aspect ratio changes from {} ({}) to {} ({}). {how} The background image is scaled by GRUB, which can stretch it: `--crop-background` crops it instead.",
            plan.design, plan.design.ratio_label(), plan.target, plan.target.ratio_label()
        ));
    }
    let smallest = sx.min(sy);
    if !ctx.fonts.is_empty() && !(0.85..=1.2).contains(&smallest) {
        warnings.push(format!(
            "Fonts cannot be scaled: they are bitmap (.pf2) files with a fixed size ({}). At {} the text may look too large or too small. \
If it does not fit, make a font for the new size with `grub-mkfont -s <size>`.",
            ctx.fonts.join(", "), plan.target
        ));
    }

    let mut edits = ctx.edits;
    edits.sort_by(|a, b| b.start.cmp(&a.start));
    let mut text = theme.src.clone();
    for e in edits {
        text.replace_range(e.start..e.end, &e.text);
    }
    let mut changes = ctx.changes;
    changes.sort_by_key(|c| c.line);
    Fitted { text, changes, warnings }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::tests_sample::NEXUS;

    fn plan(target: (u32, u32), style: Style, layout: Layout) -> Plan {
        Plan { design: Res::new(1920, 1080), target: Res::new(target.0, target.1), style, layout, scale_items: false, crop_background: false }
    }

    fn run(src: &str, p: &Plan) -> Fitted {
        fit(&Theme::parse(src).unwrap(), p)
    }

    #[test]
    fn caelestia_nexus_on_a_5_4_screen_exact() {
        let f = run(NEXUS, &plan((1280, 1024), Style::Exact, Layout::Stretch));
        for want in ["left = 20%-3", "top = 21%+3", "width = 60%+5", "height = 57%+4", "top = 89%-1", "height = 1%+1"] {
            assert!(f.text.contains(want), "missing `{want}` in:\n{}", f.text);
        }
        assert_eq!(f.changes.len(), 8);
    }

    #[test]
    fn everything_else_is_copied_untouched() {
        let f = run(NEXUS, &plan((1280, 1024), Style::Exact, Layout::Stretch));
        let a: Vec<&str> = NEXUS.lines().collect();
        let b: Vec<&str> = f.text.lines().collect();
        assert_eq!(a.len(), b.len());
        let changed = a.iter().zip(&b).filter(|(x, y)| x != y).count();
        assert_eq!(changed, 8);
        assert!(f.text.starts_with("# GRUB2 Theme - Caelestia Nexus\n"));
        assert!(f.text.contains("terminal-left: \"20%\""));
        assert!(f.text.contains("item_color = \"#72757e\""));
    }

    #[test]
    fn percent_style_keeps_whole_percentages_only() {
        let f = run(NEXUS, &plan((1280, 1024), Style::Percent, Layout::Stretch));
        for want in ["left = 20%", "top = 21%", "width = 60%", "height = 57%", "top = 89%", "height = 11"] {
            assert!(f.text.contains(want), "missing `{want}`");
        }
        assert!(!f.text.contains("%+") && !f.text.contains("%-"));
    }

    #[test]
    fn center_layout_keeps_proportions_and_centres() {
        let f = run(NEXUS, &plan((1280, 1024), Style::Exact, Layout::Center));
        assert!(f.text.contains("top = 30%-2"), "{}", f.text);
        assert!(f.text.contains("height = 40%+4"), "{}", f.text);
        assert!(f.text.contains("left = 20%-3"));
    }

    #[test]
    fn mixed_values_keep_their_percent_and_scale_the_pixels() {
        let f = run("+ label {\n left = 50%-300\n top = 10%+30\n width = 100%\n}", &plan((1280, 720), Style::Exact, Layout::Stretch));
        assert!(f.text.contains("left = 50%-200"), "{}", f.text);
        assert!(f.text.contains("top = 10%+20"), "{}", f.text);
        assert!(f.text.contains("width = 100%"));
        assert_eq!(f.changes.len(), 2);
    }

    #[test]
    fn children_are_relative_to_their_parent() {
        let src = "+ hbox {\n left = 100\n top = 100\n width = 800\n height = 400\n + label {\n  left = 50\n  top = 20\n  width = 200\n  height = 30\n }\n}";
        let p = Plan { design: Res::new(1000, 1000), target: Res::new(500, 500), style: Style::Exact, layout: Layout::Stretch, scale_items: false, crop_background: false };
        let f = run(src, &p);
        assert!(f.text.contains("left = 10%"), "{}", f.text);
        assert!(f.text.contains("width = 80%"), "{}", f.text);
        // label.left is 50 of 800 px in the design: 6.25% of a 400 px parent
        assert!(f.text.contains("left = 6%+1"), "{}", f.text);
    }

    #[test]
    fn quotes_around_a_value_are_kept() {
        let p = Plan { design: Res::new(1000, 1000), target: Res::new(500, 500), style: Style::Exact, layout: Layout::Stretch, scale_items: false, crop_background: false };
        let f = run("terminal-left: \"100\"\nterminal-width: \"60%\"\n", &p);
        assert!(f.text.contains("terminal-left: \"10%\""), "{}", f.text);
        assert!(f.text.contains("terminal-width: \"60%\""));
    }

    #[test]
    fn a_container_without_a_size_is_left_alone_and_reported() {
        let f = run("+ vbox {\n + label {\n  left = 5\n }\n}", &plan((1280, 720), Style::Exact, Layout::Stretch));
        assert!(f.changes.is_empty());
        assert!(f.warnings.iter().any(|w| w.contains("container whose size is not given")), "{:?}", f.warnings);
    }

    #[test]
    fn item_sizes_scale_only_when_asked() {
        let mut p = plan((1280, 720), Style::Exact, Layout::Stretch);
        assert!(run(NEXUS, &p).text.contains("item_height = 48"));
        p.scale_items = true;
        let f = run(NEXUS, &p);
        assert!(f.text.contains("item_height = 32"), "{}", f.text);
        assert!(f.text.contains("item_spacing = 11"));
    }

    #[test]
    fn crop_background_adds_or_replaces_the_scale_method() {
        let mut p = plan((1280, 1024), Style::Exact, Layout::Stretch);
        p.crop_background = true;
        let f = run(NEXUS, &p);
        assert!(f.text.contains("desktop-image: \"background.png\"\ndesktop-image-scale-method: \"crop\"\n"), "{}", f.text);
        let f = run("desktop-image: \"b.png\"\ndesktop-image-scale-method: \"stretch\"\n", &p);
        assert!(f.text.contains("desktop-image-scale-method: \"crop\""));
        assert!(!f.text.contains("stretch"));
    }

    #[test]
    fn warns_about_aspect_ratio_and_fonts() {
        let f = run(NEXUS, &plan((1280, 1024), Style::Exact, Layout::Stretch));
        assert!(f.warnings.iter().any(|w| w.contains("aspect ratio changes from 1920x1080 (16:9) to 1280x1024 (5:4)")));
        assert!(f.warnings.iter().any(|w| w.contains("Fonts cannot be scaled") && w.contains("Roboto Mono Regular 22")));
        let same = run(NEXUS, &plan((1920, 1080), Style::Exact, Layout::Stretch));
        assert!(same.warnings.is_empty(), "{:?}", same.warnings);
    }

    #[test]
    fn running_twice_on_the_original_gives_the_same_result() {
        let p = plan((1366, 768), Style::Exact, Layout::Stretch);
        assert_eq!(run(NEXUS, &p).text, run(NEXUS, &p).text);
    }
}
