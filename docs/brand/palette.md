# Morph palette

Dark is the default theme. The brand is charcoal, white, neon cyan, and neon magenta — the same colours as the glitch mark.

Neon cyan `#00E5FF` and neon magenta `#FF2BD6` are for fills, strokes, focus rings, and the logo. They meet WCAG AA on every dark surface. They do **not** meet AA as body text on light surfaces; the light theme uses the deep text roles `cyan-text` and `magenta-text` for links, labels, and unfilled controls.

Filled controls that use a neon or deep brand colour should use the matching `on-*` token for their label, never an assumed white or black.

Contrast ratios below use WCAG 2.x relative luminance. AA for normal text is 4.5:1. Large text and non-text UI (1.4.11) is 3:1.

## Brand (theme-independent)

| Token | Hex | Use |
| --- | --- | --- |
| charcoal | `#0D0D12` | Logo plate, dark background, on-neon text |
| white | `#FFFFFF` | Logo core, light-theme on-primary |
| cyan | `#00E5FF` | Brand primary, glitch, dark-theme fills and focus |
| magenta | `#FF2BD6` | Brand secondary, glitch, dark-theme fills |

## Dark theme (default)

| Token | Hex | Role |
| --- | --- | --- |
| background | `#0D0D12` | App chrome, scaffold |
| surface | `#16161D` | Cards, bars, sheets |
| surface-variant | `#1E1E28` | Grouped rows, inset wells, chips |
| text-primary | `#F5F6FA` | Body, titles, icons |
| text-secondary | `#A8AAB8` | Hints, metadata, inactive |
| primary | `#00E5FF` | Filled buttons, links, key accents |
| on-primary | `#0D0D12` | Text and icons on a cyan fill |
| secondary | `#FF2BD6` | Secondary fills and accents |
| on-secondary | `#0D0D12` | Text and icons on a magenta fill |
| success | `#3DFF9A` | Positive status |
| warning | `#FFC14D` | Caution status |
| error | `#FF4D6A` | Destructive status, errors |
| info | `#6B9BFF` | Informational status (distinct from cyan) |
| on-status | `#0D0D12` | Text on any status fill |
| border | `#707284` | Component outlines (3:1 against all dark surfaces) |
| selection | `#0A434D` | Text selection background (cyan at 25% on charcoal) |
| focus | `#00E5FF` | Focus ring |

### Dark contrast

| Foreground | Background | Ratio | AA |
| --- | --- | --- | --- |
| `#F5F6FA` text-primary | `#0D0D12` background | 17.95 | Pass |
| `#F5F6FA` text-primary | `#16161D` surface | 16.67 | Pass |
| `#F5F6FA` text-primary | `#1E1E28` surface-variant | 15.29 | Pass |
| `#A8AAB8` text-secondary | `#0D0D12` background | 8.41 | Pass |
| `#A8AAB8` text-secondary | `#16161D` surface | 7.81 | Pass |
| `#A8AAB8` text-secondary | `#1E1E28` surface-variant | 7.16 | Pass |
| `#00E5FF` primary | `#0D0D12` background | 12.60 | Pass |
| `#00E5FF` primary | `#16161D` surface | 11.70 | Pass |
| `#00E5FF` primary | `#1E1E28` surface-variant | 10.74 | Pass |
| `#FF2BD6` secondary | `#0D0D12` background | 6.06 | Pass |
| `#FF2BD6` secondary | `#16161D` surface | 5.63 | Pass |
| `#FF2BD6` secondary | `#1E1E28` surface-variant | 5.17 | Pass |
| `#0D0D12` on-primary | `#00E5FF` primary | 12.60 | Pass |
| `#0D0D12` on-secondary | `#FF2BD6` secondary | 6.06 | Pass |
| `#3DFF9A` success | `#0D0D12` background | 14.74 | Pass |
| `#3DFF9A` success | `#1E1E28` surface-variant | 12.56 | Pass |
| `#FFC14D` warning | `#0D0D12` background | 11.99 | Pass |
| `#FFC14D` warning | `#1E1E28` surface-variant | 10.21 | Pass |
| `#FF4D6A` error | `#0D0D12` background | 6.02 | Pass |
| `#FF4D6A` error | `#1E1E28` surface-variant | 5.13 | Pass |
| `#6B9BFF` info | `#0D0D12` background | 7.16 | Pass |
| `#6B9BFF` info | `#1E1E28` surface-variant | 6.10 | Pass |
| `#0D0D12` on-status | `#3DFF9A` success | 14.74 | Pass |
| `#0D0D12` on-status | `#FFC14D` warning | 11.99 | Pass |
| `#0D0D12` on-status | `#FF4D6A` error | 6.02 | Pass |
| `#0D0D12` on-status | `#6B9BFF` info | 7.16 | Pass |
| `#F5F6FA` text-primary | `#0A434D` selection | 10.11 | Pass |
| `#A8AAB8` text-secondary | `#0A434D` selection | 4.74 | Pass |
| `#00E5FF` focus | `#0D0D12` background | 12.60 | Pass (UI) |
| `#707284` border | `#0D0D12` background | 4.09 | Pass (UI) |
| `#707284` border | `#16161D` surface | 3.80 | Pass (UI) |
| `#707284` border | `#1E1E28` surface-variant | 3.48 | Pass (UI) |

## Light theme

| Token | Hex | Role |
| --- | --- | --- |
| background | `#F4F5F8` | App chrome, scaffold |
| surface | `#FFFFFF` | Cards, bars, sheets |
| surface-variant | `#E8E9F0` | Grouped rows, inset wells, chips |
| text-primary | `#0D0D12` | Body, titles, icons |
| text-secondary | `#4A4C5A` | Hints, metadata, inactive |
| primary | `#006E8A` | Filled buttons, links, key accents |
| on-primary | `#FFFFFF` | Text and icons on a deep-cyan fill |
| secondary | `#B0008A` | Secondary fills, links, accents |
| on-secondary | `#FFFFFF` | Text and icons on a deep-magenta fill |
| cyan-brand | `#00E5FF` | Graphics and the logo only — not text |
| magenta-brand | `#FF2BD6` | Graphics and the logo only — not text |
| success | `#00743F` | Positive status |
| warning | `#8A5A00` | Caution status |
| error | `#C4002A` | Destructive status, errors |
| info | `#1A4EC8` | Informational status |
| on-status | `#FFFFFF` | Text on any status fill |
| border | `#7A7C8A` | Component outlines (3:1 against all light surfaces) |
| selection | `#BFF8FF` | Text selection background (cyan at 25% on white) |
| focus | `#006E8A` | Focus ring |

### Light contrast

| Foreground | Background | Ratio | AA |
| --- | --- | --- | --- |
| `#0D0D12` text-primary | `#FFFFFF` surface | 19.38 | Pass |
| `#0D0D12` text-primary | `#F4F5F8` background | 17.78 | Pass |
| `#0D0D12` text-primary | `#E8E9F0` surface-variant | 16.01 | Pass |
| `#4A4C5A` text-secondary | `#FFFFFF` surface | 8.49 | Pass |
| `#4A4C5A` text-secondary | `#F4F5F8` background | 7.79 | Pass |
| `#4A4C5A` text-secondary | `#E8E9F0` surface-variant | 7.01 | Pass |
| `#006E8A` primary | `#FFFFFF` surface | 5.84 | Pass |
| `#006E8A` primary | `#F4F5F8` background | 5.35 | Pass |
| `#006E8A` primary | `#E8E9F0` surface-variant | 4.82 | Pass |
| `#B0008A` secondary | `#FFFFFF` surface | 6.54 | Pass |
| `#B0008A` secondary | `#F4F5F8` background | 6.00 | Pass |
| `#B0008A` secondary | `#E8E9F0` surface-variant | 5.40 | Pass |
| `#FFFFFF` on-primary | `#006E8A` primary | 5.84 | Pass |
| `#FFFFFF` on-secondary | `#B0008A` secondary | 6.54 | Pass |
| `#00743F` success | `#FFFFFF` surface | 5.88 | Pass |
| `#00743F` success | `#F4F5F8` background | 5.40 | Pass |
| `#00743F` success | `#E8E9F0` surface-variant | 4.86 | Pass |
| `#8A5A00` warning | `#FFFFFF` surface | 5.93 | Pass |
| `#8A5A00` warning | `#F4F5F8` background | 5.44 | Pass |
| `#8A5A00` warning | `#E8E9F0` surface-variant | 4.90 | Pass |
| `#C4002A` error | `#FFFFFF` surface | 6.21 | Pass |
| `#C4002A` error | `#F4F5F8` background | 5.70 | Pass |
| `#C4002A` error | `#E8E9F0` surface-variant | 5.13 | Pass |
| `#1A4EC8` info | `#FFFFFF` surface | 7.08 | Pass |
| `#1A4EC8` info | `#F4F5F8` background | 6.49 | Pass |
| `#1A4EC8` info | `#E8E9F0` surface-variant | 5.84 | Pass |
| `#FFFFFF` on-status | `#00743F` success | 5.88 | Pass |
| `#FFFFFF` on-status | `#8A5A00` warning | 5.93 | Pass |
| `#FFFFFF` on-status | `#C4002A` error | 6.21 | Pass |
| `#FFFFFF` on-status | `#1A4EC8` info | 7.08 | Pass |
| `#0D0D12` text-primary | `#BFF8FF` selection | 16.69 | Pass |
| `#4A4C5A` text-secondary | `#BFF8FF` selection | 7.31 | Pass |
| `#006E8A` focus | `#FFFFFF` surface | 5.84 | Pass (UI) |
| `#006E8A` focus | `#F4F5F8` background | 5.35 | Pass (UI) |
| `#006E8A` focus | `#E8E9F0` surface-variant | 4.82 | Pass (UI) |
| `#7A7C8A` border | `#FFFFFF` surface | 4.14 | Pass (UI) |
| `#7A7C8A` border | `#F4F5F8` background | 3.79 | Pass (UI) |
| `#7A7C8A` border | `#E8E9F0` surface-variant | 3.42 | Pass (UI) |

Neon brand colours on light surfaces fail AA as text and must not be used for labels or links:

| Foreground | Background | Ratio | AA |
| --- | --- | --- | --- |
| `#00E5FF` cyan | `#FFFFFF` surface | 1.54 | Fail |
| `#00E5FF` cyan | `#F4F5F8` background | 1.41 | Fail |
| `#00E5FF` cyan | `#E8E9F0` surface-variant | 1.27 | Fail |
| `#FF2BD6` magenta | `#FFFFFF` surface | 3.20 | Fail (large text only) |
| `#FF2BD6` magenta | `#F4F5F8` background | 2.93 | Fail |
| `#FF2BD6` magenta | `#E8E9F0` surface-variant | 2.64 | Fail |

## Machine-readable tokens

- `tokens.json` — brand plus dark and light themes, for the Jaspr site and other tools
- `tokens.css` — the same values as CSS custom properties, defaulting to dark, with `[data-theme="light"]` overrides
- `app/lib/src/theme/morph_theme.dart` — `MorphColors`, `MorphTheme.dark`, and `MorphTheme.light` (`ThemeData` / `ColorScheme`)
