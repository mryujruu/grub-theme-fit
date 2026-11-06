# Changelog

## 0.1.0

First release.

- `auto`: detects the screen (GRUB_GFXMODE, then the connected display) and fits the theme to it.
- `manual`: you choose the resolution from a menu of known sizes, or pass `--resolution WxH`.
- `resolutions`: lists the known sizes by aspect ratio.
- Converts pixel `left`, `top`, `width` and `height` to percentages (`20%-3`), including nested components and the `terminal-*` box. Values already in percent are kept.
- Works out the size the theme was drawn for from its background image and layout, and asks when it is not sure.
- `--style exact|percent`, `--layout stretch|center`, `--scale-items`, `--crop-background`.
- Preview by default. `--write` keeps the original as `theme.txt.fit-orig`, so running again never compounds; `restore` puts it back; `status` shows what was done.
- Rewrites only the values that change; comments, spacing and everything else stay byte for byte the same.
- 46 tests (unit and end to end). No dependencies.
