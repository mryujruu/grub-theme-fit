# grub-theme-fit

Make a GRUB theme fit your screen.

Most GRUB themes are drawn for one resolution (usually 1920x1080) and place everything in **pixels**. On another screen the menu ends up in the wrong place, too narrow, or off the edge. `grub-theme-fit` rewrites the theme's layout for your resolution:

- **`auto`**: finds your screen by itself.
- **`manual`**: you pick from a list of all the common resolutions (or type one).

Written in Rust. No dependencies, one small binary. Arabic version: [README.ar.md](README.ar.md)

```
$ grub-theme-fit manual caelestia-nexus --resolution 1280x1024

 grub-theme-fit 0.1.0

 Theme        /boot/grub/themes/caelestia-nexus/theme.txt
 Drawn for    1920x1080 (16:9) guessed: the background is 3840x2160 (16:9), and the layout
              reaches 1540x972, so 1920x1080 is the smallest 16:9 size it fits
 Fit to       1280x1024 (5:4) from --resolution
 Mode         stretch, exact percentages

 8 values change
   boot_menu.left                    380  →  20%-3  line 12
   boot_menu.top                     230  →  21%+3  line 13
   boot_menu.width                  1160  →  60%+5  line 14
   boot_menu.height                  620  →  57%+4  line 15
   progress_bar#__timeout__.left     380  →  20%-3  line 28
   ...

 ⚠ The aspect ratio changes from 1920x1080 (16:9) to 1280x1024 (5:4). ...
 ⚠ Fonts cannot be scaled: they are bitmap (.pf2) files with a fixed size ...

 Preview only: nothing was changed.
```

## Install

You need Rust (`sudo pacman -S rust` on Arch).

```sh
cargo install --git https://github.com/mryujruu/grub-theme-fit
```

or from a clone: `cargo build --release`, and the binary is `target/release/grub-theme-fit`.

## Use

```sh
grub-theme-fit auto                          # the theme in GRUB_THEME, your screen, preview only
grub-theme-fit auto  caelestia-nexus --write # apply it (needs sudo for /boot)
grub-theme-fit manual                        # pick the resolution from a menu
grub-theme-fit manual mytheme --resolution 1366x768
grub-theme-fit resolutions --ratio 16:9      # list the known sizes
grub-theme-fit status                        # was this theme fitted, and to what?
grub-theme-fit restore                       # put the original back
```

`THEME` can be a `theme.txt`, a theme folder, or a theme name found in `/boot/grub/themes`. Without it, the theme named by `GRUB_THEME` in `/etc/default/grub` is used.

`--write` into `/boot` needs root. The tool then prints the exact `sudo ...` command to run.

### The two modes

| | `auto` | `manual` |
|---|---|---|
| Screen size | `GRUB_GFXMODE` in `/etc/default/grub` (the size GRUB itself uses), otherwise the first connected display in `/sys/class/drm` | a numbered menu of known sizes, or `--resolution WxH` |
| When to use | your screen is what GRUB uses | GRUB runs at a different size than your desktop, you are preparing another machine, or detection fails |

The known sizes (`grub-theme-fit resolutions`): 4:3 from 640x480 to 2048x1536, 5:4 (1280x1024, 2560x2048), 3:2, 16:10 from 1280x800 to 3840x2400, 16:9 from 1280x720 to 7680x4320, and the ultrawides 21:9 and 32:9. Any other size works with `--resolution`.

### Options

| Option | What it does |
|---|---|
| `--design WxH` | The size the theme was drawn for. Normally guessed, see below. |
| `--style exact` (default) | `20%-3`: percent plus a few pixels, precise at this resolution. |
| `--style percent` | `20%`: whole percentages only. Works at any resolution, but less precise. |
| `--layout stretch` (default) | Each axis scales on its own. |
| `--layout center` | The layout keeps its proportions, is scaled by one factor and centred. Best when the aspect ratio differs a lot. |
| `--scale-items` | Also scales `item_height`, `item_padding`, `item_spacing` and icon sizes. |
| `--crop-background` | Sets `desktop-image-scale-method` to `crop`, so GRUB crops the background instead of stretching it. |
| `--write` | Applies the change. Without it you only get a preview. |
| `-y`, `--yes` | Accept a weak guess of the design size without stopping. |

## How it works

GRUB's theme format lets `left`, `top`, `width` and `height` be pixels (`380`), a percentage (`20%`), or both (`50%-300`). Percentages are relative to the parent. This tool converts the pixel values, relative to the size the theme was drawn for, into values for your screen.

- **Lossless.** Only the numbers that change are rewritten. Comments, spacing, colours, fonts and image names are copied as they were.
- **Nested components** (`hbox`, `vbox`, `canvas`) are converted relative to their parent. If a container has no width and height, its children are left as they are and the tool says so.
- **Already-relative values** (`20%`) stay. A theme that only uses percentages is reported as "nothing to convert".
- **Safe to repeat.** The first `--write` saves the original as `theme.txt.fit-orig` (and the settings in `theme.txt.fit-meta`). Every later run starts from the original, so running it again for another resolution does not stack changes. `restore` puts the original back.
- **The design size is guessed** from the background image's aspect ratio and how far the layout reaches. When the guess is weak (for example there is no background image), it stops and asks for `--design WxH` instead of guessing silently.
- It never edits `/etc/default/grub` and never runs `grub-mkconfig`. GRUB reads `theme.txt` at boot.

## What it cannot do

- **Fonts.** GRUB fonts (`.pf2`) are bitmaps with a fixed size. At a very different resolution the text can look too big or too small. The tool warns you, and `grub-mkfont -s <size>` can make a font for the new size.
- **Images.** Icons and the pieces of a nine-slice box (`select_*.png`) keep their size. `--scale-items` changes the sizes in the layout, not the images.
- **A different shape.** Percentages cannot turn a 16:9 design into a good 5:4 design. `--layout center` and `--crop-background` help, and so does picking a theme made for your shape.
- **Item sizes.** GRUB does not accept percentages for `item_height` and the other item settings, so they stay in pixels (or scale with `--scale-items`).

## Status

Version 0.1.0. The conversion is covered by 46 tests (unit and end to end), including a real layout (Caelestia Nexus, converted for a 1280x1024 screen). I have not yet booted a converted theme on many machines, so please report themes that go wrong, with the `theme.txt` attached.

## Development

```sh
cargo test
cargo build --release
```

## License

MIT
