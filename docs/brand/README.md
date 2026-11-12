# Morph brand

Glitch direction: a heavy white M with horizontal RGB slices in cyan
`#00E5FF` and magenta `#FF2BD6`, on charcoal `#0D0D12`.

The SVGs in `svg/` are the source of truth. They are drawn on a 1024
viewBox snapped to a 32-unit grid so the white core of the M stays
readable at 16px (64 units per device pixel). Raster assets were
rendered from those SVGs with cairosvg.

## Mark and wordmark

| File | Use |
| --- | --- |
| `svg/morph-icon.svg` | App icon with a rounded-square plate. Previews, Linux, Windows, favicon. |
| `svg/morph-icon-square.svg` | Full-bleed square, no transparency. iOS and macOS AppIcon catalogs so the OS can apply its own mask. |
| `svg/morph-mark.svg` | Colour mark, transparent background. |
| `svg/morph-mark-mono.svg` | Same geometry, including the displaced slices, in `currentColor` (default charcoal). |
| `svg/morph-mark-mono-white.svg` | The monochrome mark with `color="#FFFFFF"` for dark backgrounds. |
| `svg/morph-wordmark.svg` | Rounded icon + geometric mono "morph" in white, for dark backgrounds. |
| `svg/morph-wordmark-light.svg` | The same lockup with charcoal type, for light backgrounds. |
| `svg/favicon.svg` | Alias of the rounded icon, for the future Jaspr site. |

PNG renders of the same files live in `png/`. Platform-sized icons are
wired into the Flutter app under `app/android`, `app/ios`, `app/macos`,
`app/windows`, and `app/linux`.

## Colour

Dark is the default theme. See [palette.md](palette.md) for roles,
WCAG AA contrast ratios, and the light-theme deep cyan / magenta text
roles. Machine-readable values are in `tokens.css` and `tokens.json`.
The Flutter `ThemeData` lives at `app/lib/src/theme/morph_theme.dart`.
