mod detect;
mod fit;
mod resolutions;
mod theme;
mod value;

use detect::{Confidence, DRM_DIR, GRUB_DEFAULT, THEME_DIRS};
use fit::{Fitted, Layout, Plan, Style};
use resolutions::{Res, KNOWN};
use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use theme::Theme;

const VERSION: &str = env!("CARGO_PKG_VERSION");

const HELP: &str = "\
grub-theme-fit: make a GRUB theme fit your screen

USAGE
  grub-theme-fit auto   [THEME] [options]   detect the screen and fit the theme to it
  grub-theme-fit manual [THEME] [options]   you choose the resolution (menu of known sizes, or --resolution)
  grub-theme-fit resolutions [--ratio 16:9] list the known resolutions
  grub-theme-fit status  [THEME]            show whether a theme was fitted, and to what
  grub-theme-fit restore [THEME]            put the original theme.txt back

THEME is a theme.txt, a theme folder, or a theme name found in /boot/grub/themes.
Without it, the theme named by GRUB_THEME in /etc/default/grub is used.

OPTIONS
  --resolution WxH     manual: the screen size to fit to, for example 1280x1024
  --design WxH         the size the theme was drawn for (guessed from the background image and layout)
  --style exact|percent
                       exact (default): `20%-3`, precise at this resolution
                       percent: whole percentages, works at any resolution but less precise
  --layout stretch|center
                       stretch (default): each axis scales on its own
                       center: keep the layout's proportions and centre it
  --scale-items        also scale item_height, item_padding, item_spacing and icon sizes
  --crop-background    set desktop-image-scale-method to crop, so the background is not stretched
  --write              apply the change. Without it nothing is changed: you only see a preview
  -y, --yes            do not ask questions (accept a weak design-size guess)
  -h, --help, -V, --version

The original theme.txt is kept as theme.txt.fit-orig, and every run starts from it,
so running again with another resolution never compounds. `restore` undoes everything.
Writing into /boot usually needs sudo; the preview does not.";

#[derive(Default)]
struct Opts {
    theme: Option<String>,
    design: Option<Res>,
    resolution: Option<Res>,
    style: Option<Style>,
    layout: Option<Layout>,
    scale_items: bool,
    crop: bool,
    write: bool,
    yes: bool,
    ratio: Option<String>,
    grub_default: Option<PathBuf>,
    drm_dir: Option<PathBuf>,
}

fn usage_error(msg: &str) -> i32 {
    eprintln!("grub-theme-fit: {msg}\nTry `grub-theme-fit --help`.");
    2
}

fn parse_res(flag: &str, v: &str) -> Result<Res, String> {
    Res::parse(v).ok_or_else(|| format!("{flag} needs a size like 1280x1024, not `{v}`"))
}

fn parse_opts(args: &[String]) -> Result<Opts, String> {
    let mut o = Opts::default();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let (flag, inline) = match a.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
            _ => (a.clone(), None),
        };
        let value = |i: &mut usize| -> Result<String, String> {
            if let Some(v) = &inline {
                return Ok(v.clone());
            }
            *i += 1;
            args.get(*i).cloned().ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--resolution" => o.resolution = Some(parse_res(&flag, &value(&mut i)?)?),
            "--design" => o.design = Some(parse_res(&flag, &value(&mut i)?)?),
            "--style" => {
                o.style = Some(match value(&mut i)?.as_str() {
                    "exact" => Style::Exact,
                    "percent" => Style::Percent,
                    v => return Err(format!("--style is exact or percent, not `{v}`")),
                })
            }
            "--layout" => {
                o.layout = Some(match value(&mut i)?.as_str() {
                    "stretch" => Layout::Stretch,
                    "center" | "centre" => Layout::Center,
                    v => return Err(format!("--layout is stretch or center, not `{v}`")),
                })
            }
            "--ratio" => o.ratio = Some(value(&mut i)?),
            "--grub-default" => o.grub_default = Some(PathBuf::from(value(&mut i)?)),
            "--drm-dir" => o.drm_dir = Some(PathBuf::from(value(&mut i)?)),
            "--scale-items" => o.scale_items = true,
            "--crop-background" => o.crop = true,
            "--write" => o.write = true,
            "-y" | "--yes" => o.yes = true,
            f if f.starts_with('-') => return Err(format!("unknown option `{f}`")),
            _ => {
                if o.theme.is_some() {
                    return Err(format!("only one theme can be given, found a second: `{a}`"));
                }
                o.theme = Some(a.clone());
            }
        }
        i += 1;
    }
    Ok(o)
}

struct Ui {
    color: bool,
}

impl Ui {
    fn new() -> Ui {
        Ui { color: io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none() }
    }
    fn c(&self, code: &str, s: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }
    fn bold(&self, s: &str) -> String {
        self.c("1", s)
    }
    fn dim(&self, s: &str) -> String {
        self.c("2", s)
    }
}

fn orig_path(theme: &Path) -> PathBuf {
    let name = theme.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "theme.txt".into());
    theme.with_file_name(format!("{name}.fit-orig"))
}

fn meta_path(theme: &Path) -> PathBuf {
    let name = theme.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "theme.txt".into());
    theme.with_file_name(format!("{name}.fit-meta"))
}

fn read_meta(theme: &Path) -> Option<std::collections::HashMap<String, String>> {
    let text = fs::read_to_string(meta_path(theme)).ok()?;
    Some(text.lines().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect())
}

fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.fit-tmp"));
    fs::write(&tmp, text)?;
    if let Ok(m) = fs::metadata(path) {
        let _ = fs::set_permissions(&tmp, m.permissions());
    }
    let result = fs::rename(&tmp, path);
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn ask(prompt: &str) -> Option<String> {
    print!("{prompt}");
    let _ = io::stdout().flush();
    let mut line = String::new();
    match io::stdin().lock().read_line(&mut line) {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(line.trim().to_string()),
    }
}

fn print_resolutions(ratio: Option<&str>) {
    for (r, list) in KNOWN {
        if ratio.is_some_and(|x| x != *r) {
            continue;
        }
        let sizes: Vec<String> = list.iter().map(|(w, h)| format!("{w}x{h}")).collect();
        println!("  {:<6} {}", r, sizes.join("  "));
    }
}

fn pick_resolution() -> Result<Res, String> {
    let all = resolutions::all_known();
    println!("Pick your screen resolution:\n");
    let mut n = 0;
    for (ratio, list) in KNOWN {
        println!("  {ratio}");
        for (w, h) in *list {
            n += 1;
            println!("    {n:>2}) {w}x{h}");
        }
    }
    println!();
    let answer = ask("Number, or type a size like 1366x768 (q to quit): ").ok_or("no answer")?;
    if answer.eq_ignore_ascii_case("q") || answer.is_empty() {
        return Err("cancelled".into());
    }
    if let Ok(k) = answer.parse::<usize>() {
        return all.get(k.wrapping_sub(1)).copied().ok_or_else(|| format!("there is no number {k}"));
    }
    Res::parse(&answer).ok_or_else(|| format!("`{answer}` is not a number from the list or a size like 1280x1024"))
}

fn sudo_hint() -> String {
    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "grub-theme-fit".into());
    let args: Vec<String> = std::env::args().skip(1).collect();
    format!("sudo {exe} {}", args.join(" "))
}

fn io_message(what: &str, e: &io::Error) -> String {
    if e.kind() == io::ErrorKind::PermissionDenied {
        format!("{what}: permission denied. Themes under /boot belong to root, so run it with sudo:\n  {}", sudo_hint())
    } else {
        format!("{what}: {e}")
    }
}

fn run_fit(mode: &str, o: Opts) -> Result<i32, String> {
    let ui = Ui::new();
    let grub_default = o.grub_default.clone().unwrap_or_else(|| PathBuf::from(GRUB_DEFAULT));
    let drm_dir = o.drm_dir.clone().unwrap_or_else(|| PathBuf::from(DRM_DIR));
    if mode == "auto" && o.resolution.is_some() {
        return Err("--resolution is for `manual`. `auto` detects the screen by itself".into());
    }

    let theme_path = detect::find_theme(o.theme.as_deref(), &grub_default, THEME_DIRS)?;
    let theme_dir = theme_path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let orig = orig_path(&theme_path);
    let from_orig = orig.is_file();
    let src = fs::read_to_string(if from_orig { &orig } else { &theme_path }).map_err(|e| io_message("could not read the theme", &e))?;
    let theme = Theme::parse(&src).map_err(|e| format!("{} is not a theme this tool can read: {e}", theme_path.display()))?;

    if !detect::has_pixel_values(&theme) && !o.crop {
        println!("{} already uses percentages only, so it fits every resolution. Nothing to convert.", theme_path.display());
        return Ok(0);
    }

    // the screen
    let (target, target_src) = match mode {
        "auto" => {
            let d = detect::detect_screen(&grub_default, &drm_dir)?;
            (d.res, d.source)
        }
        _ => match o.resolution {
            Some(r) => (r, "--resolution".to_string()),
            None => {
                if !io::stdin().is_terminal() {
                    return Err("manual needs --resolution WxH when there is no terminal to ask in (see `grub-theme-fit resolutions`)".into());
                }
                (pick_resolution()?, "your choice".to_string())
            }
        },
    };

    // the size the theme was drawn for
    let meta = read_meta(&theme_path).filter(|_| from_orig);
    let (design, design_src, weak) = if let Some(d) = o.design {
        (d, "--design".to_string(), false)
    } else if let Some(d) = meta.as_ref().and_then(|m| m.get("design")).and_then(|v| Res::parse(v)) {
        (d, "the earlier run (theme.txt.fit-meta)".to_string(), false)
    } else {
        match detect::guess_design(&theme, &theme_dir) {
            Some(g) => {
                let weak = g.confidence == Confidence::Weak;
                (g.res, format!("guessed: {}", g.reason), weak)
            }
            None => return Err("could not tell what resolution this theme was drawn for. Give it with --design WxH".into()),
        }
    };
    if weak && !o.yes {
        return Err(format!(
            "I am not sure what resolution this theme was drawn for (my guess is {design}: {design_src}).\n\
Run it again with --design WxH to say so, or with --yes to accept the guess."
        ));
    }

    let plan = Plan {
        design,
        target,
        style: o.style.unwrap_or(Style::Exact),
        layout: o.layout.unwrap_or(Layout::Stretch),
        scale_items: o.scale_items,
        crop_background: o.crop,
    };
    let fitted = fit::fit(&theme, &plan);
    print_report(&ui, &theme_path, &plan, &design_src, &target_src, from_orig, &fitted);

    if fitted.changes.is_empty() {
        println!("\n {}", ui.dim("Nothing needs to change at this resolution."));
        return Ok(0);
    }
    if !o.write {
        println!("\n {}", ui.bold("Preview only: nothing was changed."));
        println!(" Add --write to apply it{}", if theme_path.starts_with("/boot") || theme_path.starts_with("/usr") { " (this theme is in a system folder, so use sudo)." } else { "." });
        return Ok(0);
    }

    if !from_orig {
        fs::copy(&theme_path, &orig).map_err(|e| io_message("could not back up the original", &e))?;
    }
    write_atomic(&theme_path, &fitted.text).map_err(|e| io_message("could not write the theme", &e))?;
    let meta_text = format!(
        "design={}\ntarget={}\nstyle={}\nlayout={}\ntool=grub-theme-fit {VERSION}\n",
        plan.design,
        plan.target,
        if plan.style == Style::Exact { "exact" } else { "percent" },
        if plan.layout == Layout::Stretch { "stretch" } else { "center" },
    );
    fs::write(meta_path(&theme_path), meta_text).map_err(|e| io_message("could not write the notes file", &e))?;
    println!("\n {} {}", ui.bold("Done."), format!("Written to {}", theme_path.display()));
    println!(" The original is kept as {}", orig.display());
    println!(" Undo with: grub-theme-fit restore {}", theme_path.display());
    println!(" {}", ui.dim("GRUB reads theme.txt at boot, so no grub-mkconfig is needed. If the screen looks wrong, check GRUB_GFXMODE in /etc/default/grub."));
    Ok(0)
}

fn print_report(ui: &Ui, theme: &Path, plan: &Plan, design_src: &str, target_src: &str, from_orig: bool, f: &Fitted) {
    println!("\n {}\n", ui.bold(&format!("grub-theme-fit {VERSION}")));
    println!(" {:<13}{}", "Theme", theme.display());
    println!(" {:<13}{} ({}) {}", "Drawn for", plan.design, plan.design.ratio_label(), ui.dim(design_src));
    println!(" {:<13}{} ({}) {}", "Fit to", plan.target, plan.target.ratio_label(), ui.dim(&format!("from {target_src}")));
    println!(
        " {:<13}{}, {}{}",
        "Mode",
        if plan.layout == Layout::Stretch { "stretch" } else { "center" },
        if plan.style == Style::Exact { "exact percentages" } else { "whole percentages" },
        if from_orig { ui.dim("   (starting from the saved original)") } else { String::new() }
    );
    println!();
    if f.changes.is_empty() {
        return;
    }
    println!(" {}", ui.bold(&format!("{} value{} change", f.changes.len(), if f.changes.len() == 1 { "" } else { "s" })));
    let w = f.changes.iter().map(|c| c.name.len()).max().unwrap_or(0);
    let ow = f.changes.iter().map(|c| c.old.len()).max().unwrap_or(0);
    for c in &f.changes {
        println!("   {:<w$}  {:>ow$}  →  {}  {}", c.name, c.old, ui.c("32", &c.new), ui.dim(&format!("line {}", c.line)));
    }
    for w in &f.warnings {
        println!("\n {} {}", ui.c("33;1", "⚠"), wrap(w, 78));
    }
}

fn wrap(s: &str, width: usize) -> String {
    let mut out = String::new();
    let mut line = 0;
    for word in s.split_whitespace() {
        if line + word.len() + 1 > width && line > 0 {
            out.push_str("\n   ");
            line = 0;
        } else if line > 0 {
            out.push(' ');
            line += 1;
        }
        out.push_str(word);
        line += word.len();
    }
    out
}

fn run_status(o: Opts) -> Result<i32, String> {
    let grub_default = o.grub_default.clone().unwrap_or_else(|| PathBuf::from(GRUB_DEFAULT));
    let path = detect::find_theme(o.theme.as_deref(), &grub_default, THEME_DIRS)?;
    println!("Theme   {}", path.display());
    match read_meta(&path).filter(|_| orig_path(&path).is_file()) {
        Some(m) => {
            println!("Fitted  yes");
            for k in ["design", "target", "style", "layout", "tool"] {
                if let Some(v) = m.get(k) {
                    println!("  {k:<8}{v}");
                }
            }
            println!("Original  {}", orig_path(&path).display());
        }
        None if orig_path(&path).is_file() => println!("Fitted  yes (the notes file is missing); original: {}", orig_path(&path).display()),
        None => println!("Fitted  no, this is the original theme"),
    }
    if let Ok(d) = detect::detect_screen(&grub_default, &o.drm_dir.unwrap_or_else(|| PathBuf::from(DRM_DIR))) {
        println!("Screen  {} ({})", d.res, d.source);
    }
    Ok(0)
}

fn run_restore(o: Opts) -> Result<i32, String> {
    let grub_default = o.grub_default.clone().unwrap_or_else(|| PathBuf::from(GRUB_DEFAULT));
    let path = detect::find_theme(o.theme.as_deref(), &grub_default, THEME_DIRS)?;
    let orig = orig_path(&path);
    if !orig.is_file() {
        println!("{} has not been fitted (no {}), nothing to restore.", path.display(), orig.display());
        return Ok(0);
    }
    let text = fs::read(&orig).map_err(|e| io_message("could not read the saved original", &e))?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.fit-tmp"));
    fs::write(&tmp, &text).map_err(|e| io_message("could not write the theme", &e))?;
    if let Ok(m) = fs::metadata(&orig) {
        let _ = fs::set_permissions(&tmp, m.permissions());
    }
    fs::rename(&tmp, &path).map_err(|e| io_message("could not put the original back", &e))?;
    let _ = fs::remove_file(&orig);
    let _ = fs::remove_file(meta_path(&path));
    println!("Restored the original {}", path.display());
    Ok(0)
}

/// Parse the options (a mistake there is a usage error, exit 2), then run the command.
fn with_opts(args: &[String], f: impl FnOnce(Opts) -> Result<i32, String>) -> i32 {
    match parse_opts(args) {
        Err(e) => usage_error(&e),
        Ok(o) => f(o).unwrap_or_else(|e| {
            eprintln!("grub-theme-fit: {e}");
            if e.contains("--resolution WxH when") { 2 } else { 1 }
        }),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        None | Some("-h") | Some("--help") | Some("help") => {
            println!("{HELP}");
            0
        }
        Some("-V") | Some("--version") => {
            println!("grub-theme-fit {VERSION}");
            0
        }
        Some(m @ ("auto" | "manual")) => with_opts(&args[1..], |o| run_fit(m, o)),
        Some("resolutions") => with_opts(&args[1..], |o| {
            print_resolutions(o.ratio.as_deref());
            Ok(0)
        }),
        Some("status") => with_opts(&args[1..], run_status),
        Some("restore") => with_opts(&args[1..], run_restore),
        Some(other) => usage_error(&format!("unknown command `{other}`")),
    };
    std::process::exit(code);
}
