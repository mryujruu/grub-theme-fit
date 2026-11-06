//! End to end: run the real binary on a theme in a temporary folder.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const NEXUS: &str = include_str!("nexus.txt");
const BIN: &str = env!("CARGO_BIN_EXE_grub-theme-fit");

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("gtf-cli-{}-{}", name, std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let mut v = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, b'I', b'H', b'D', b'R'];
    v.extend(w.to_be_bytes());
    v.extend(h.to_be_bytes());
    v.extend([8, 2, 0, 0, 0]);
    v
}

fn theme_dir(name: &str) -> PathBuf {
    let d = tmp(name);
    fs::write(d.join("theme.txt"), NEXUS).unwrap();
    fs::write(d.join("background.png"), png(3840, 2160)).unwrap();
    d
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).env("NO_COLOR", "1").stdin(Stdio::null()).output().unwrap()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn read(p: &Path) -> String {
    fs::read_to_string(p).unwrap()
}

#[test]
fn preview_shows_the_changes_and_touches_nothing() {
    let d = theme_dir("preview");
    let o = run(&["manual", d.to_str().unwrap(), "--resolution", "1280x1024"]);
    let s = out(&o);
    assert!(o.status.success(), "{s}{}", err(&o));
    assert!(s.contains("Drawn for    1920x1080 (16:9)"), "{s}");
    assert!(s.contains("Fit to       1280x1024 (5:4)"), "{s}");
    assert!(s.contains("20%-3") && s.contains("89%-1"), "{s}");
    assert!(s.contains("Preview only: nothing was changed."));
    assert!(s.contains("aspect ratio changes"));
    assert_eq!(read(&d.join("theme.txt")), NEXUS);
    assert!(!d.join("theme.txt.fit-orig").exists());
}

#[test]
fn write_keeps_the_original_and_restore_gives_it_back_byte_for_byte() {
    let d = theme_dir("write");
    let t = d.join("theme.txt");
    let o = run(&["manual", t.to_str().unwrap(), "--resolution", "1280x1024", "--write"]);
    assert!(o.status.success(), "{}{}", out(&o), err(&o));
    let fitted = read(&t);
    assert!(fitted.contains("left = 20%-3") && fitted.contains("height = 1%+1"));
    assert_eq!(read(&d.join("theme.txt.fit-orig")), NEXUS);
    assert!(read(&d.join("theme.txt.fit-meta")).contains("design=1920x1080"));

    let o = run(&["restore", t.to_str().unwrap()]);
    assert!(o.status.success());
    assert_eq!(read(&t), NEXUS);
    assert!(!d.join("theme.txt.fit-orig").exists() && !d.join("theme.txt.fit-meta").exists());
}

#[test]
fn running_again_for_another_resolution_starts_from_the_original() {
    let d = theme_dir("again");
    let t = d.join("theme.txt");
    run(&["manual", t.to_str().unwrap(), "--resolution", "1280x1024", "--write"]);
    run(&["manual", t.to_str().unwrap(), "--resolution", "1366x768", "--write"]);
    let twice = read(&t);

    let fresh = theme_dir("again-fresh");
    run(&["manual", fresh.join("theme.txt").to_str().unwrap(), "--resolution", "1366x768", "--write"]);
    assert_eq!(twice, read(&fresh.join("theme.txt")), "a second run must not compound the first");
    assert_eq!(read(&d.join("theme.txt.fit-orig")), NEXUS);
}

#[test]
fn auto_finds_the_screen_from_grub_gfxmode() {
    let d = theme_dir("auto");
    let g = d.join("grub");
    fs::write(&g, "GRUB_GFXMODE=1280x1024,auto\n").unwrap();
    let o = run(&["auto", d.to_str().unwrap(), "--grub-default", g.to_str().unwrap()]);
    let s = out(&o);
    assert!(o.status.success(), "{s}{}", err(&o));
    assert!(s.contains("Fit to       1280x1024 (5:4) from GRUB_GFXMODE"), "{s}");
}

#[test]
fn auto_falls_back_to_the_connected_display() {
    let d = theme_dir("drm");
    let drm = d.join("drm").join("card0-HDMI-A-1");
    fs::create_dir_all(&drm).unwrap();
    fs::write(drm.join("status"), "connected").unwrap();
    fs::write(drm.join("modes"), "1600x900\n").unwrap();
    let g = d.join("grub-without-gfxmode");
    fs::write(&g, "GRUB_TIMEOUT=5\n").unwrap();
    let o = run(&["auto", d.to_str().unwrap(), "--grub-default", g.to_str().unwrap(), "--drm-dir", d.join("drm").to_str().unwrap()]);
    let s = out(&o);
    assert!(s.contains("1600x900 (16:9) from the connected display card0-HDMI-A-1"), "{s}{}", err(&o));
}

#[test]
fn auto_uses_the_theme_named_by_grub_theme() {
    let d = theme_dir("gtheme");
    let g = d.join("grub");
    fs::write(&g, format!("GRUB_GFXMODE=1024x768\nGRUB_THEME=\"{}\"\n", d.join("theme.txt").display())).unwrap();
    let o = run(&["auto", "--grub-default", g.to_str().unwrap()]);
    assert!(o.status.success(), "{}{}", out(&o), err(&o));
    assert!(out(&o).contains("1024x768"));
}

#[test]
fn manual_without_a_terminal_needs_a_resolution() {
    let d = theme_dir("noterm");
    let o = run(&["manual", d.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).contains("--resolution WxH"));
}

#[test]
fn an_unsure_design_size_is_not_guessed_silently() {
    let d = tmp("weak");
    fs::write(d.join("theme.txt"), NEXUS).unwrap(); // no background image to confirm the guess
    let t = d.join("theme.txt");
    let o = run(&["manual", t.to_str().unwrap(), "--resolution", "1280x1024"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("--design WxH") && err(&o).contains("--yes"), "{}", err(&o));
    let o = run(&["manual", t.to_str().unwrap(), "--resolution", "1280x1024", "--design", "1920x1080"]);
    assert!(o.status.success());
}

#[test]
fn a_theme_that_is_already_in_percentages_is_left_alone() {
    let d = tmp("pct");
    fs::write(d.join("theme.txt"), "+ boot_menu { left = 10% top = 20% width = 80% height = 50% }\n").unwrap();
    let o = run(&["manual", d.to_str().unwrap(), "--resolution", "1280x1024"]);
    assert!(out(&o).contains("Nothing to convert"));
}

#[test]
fn a_broken_theme_is_reported_with_its_line() {
    let d = tmp("broken");
    fs::write(d.join("theme.txt"), "+ boot_menu {\n left = 10\n").unwrap();
    let o = run(&["manual", d.to_str().unwrap(), "--resolution", "1280x1024", "--design", "1920x1080"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("not a theme this tool can read"), "{}", err(&o));
}

#[test]
fn the_resolutions_list_has_the_known_sizes_and_can_filter() {
    let o = run(&["resolutions"]);
    let s = out(&o);
    assert!(s.contains("1280x1024") && s.contains("3440x1440") && s.contains("5:4"));
    let o = run(&["resolutions", "--ratio", "5:4"]);
    assert!(out(&o).contains("1280x1024") && !out(&o).contains("1920x1080"));
}

#[test]
fn bad_input_gives_a_usage_error() {
    assert_eq!(run(&["frobnicate"]).status.code(), Some(2));
    assert_eq!(run(&["manual", "x", "--resolution", "wide"]).status.code(), Some(2));
    assert_eq!(run(&["manual", "--bogus"]).status.code(), Some(2));
    assert!(run(&["--version"]).status.success());
    assert!(out(&run(&["--help"])).contains("USAGE"));
}

#[test]
fn style_layout_and_crop_options_work_from_the_command_line() {
    let d = theme_dir("opts");
    let o = run(&["manual", d.to_str().unwrap(), "--resolution", "1280x1024", "--style", "percent", "--layout", "center", "--crop-background", "--scale-items"]);
    let s = out(&o);
    assert!(o.status.success(), "{s}{}", err(&o));
    assert!(s.contains("center, whole percentages"), "{s}");
    assert!(s.contains("desktop-image-scale-method"), "{s}");
    assert!(s.contains("boot_menu.item_height"), "{s}");
}
