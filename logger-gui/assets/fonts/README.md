# Bundled GUI fonts

These fonts are embedded into `logger-gui` with `include_bytes!` (see
`FONT_FILES` in `src/panes/style.rs`) and registered with iced at startup.
Embedding them makes the GUI look the same on every machine instead of
depending on whichever sans-serif, monospace or emoji fonts the system has.
The reasoning is in `docs/gui-visual-design.md`.

| File | Family / weight | Source | License |
|---|---|---|---|
| `Inter-Regular.ttf` | Inter 400 | rsms/inter v3.19, "Inter Hinted for Windows/Desktop" | SIL OFL 1.1 (`Inter-LICENSE.txt`) |
| `Inter-Medium.ttf` | Inter 500 | same | same |
| `Inter-SemiBold.ttf` | Inter 600 | same | same |
| `JetBrainsMono-Regular.ttf` | JetBrains Mono 400 | JetBrains/JetBrainsMono v2.304, `fonts/ttf/` | SIL OFL 1.1 (`JetBrainsMono-OFL.txt`) |
| `JetBrainsMono-Bold.ttf` | JetBrains Mono 700 | same | same |
| `lucide.ttf` | lucide (icons) | npm `lucide-static@0.460.0`, `font/lucide.ttf` | ISC (`lucide-LICENSE.txt`) |

## Subsetting

Every file is subset with `pyftsubset` from fontTools, which brings the
total down to about 400 KB from about 3.5 MB:

```sh
U="U+0020-007E,U+00A0-00FF,U+0131,U+0152-0153,U+2010-2027,U+2030-203A,U+2190-2195,U+2212,U+2713,U+2715,U+26A0,U+25B2-25BC,U+2264-2265"

# Inter: keep every layout feature (kerning, contextual alternates) and all
# name IDs. Name ID 16 (typographic family = "Inter") is what lets the
# Medium and SemiBold files resolve as weights of one "Inter" family.
for w in Regular Medium SemiBold; do
  pyftsubset "Inter Hinted for Windows/Desktop/Inter-$w.ttf" \
    --unicodes="$U" --layout-features='*' --name-IDs='*' \
    --output-file=Inter-$w.ttf
done

for w in Regular Bold; do
  pyftsubset fonts/ttf/JetBrainsMono-$w.ttf \
    --unicodes="$U" --layout-features='kern,mark,mkmk,zero' \
    --output-file=JetBrainsMono-$w.ttf
done

# Lucide: only the icons in style::Icon.
#   x e1b2, palette e1dd, circle-help e082, download e0b6,
#   a-arrow-down e58a, a-arrow-up e58b, triangle-alert e193, check e070
pyftsubset lucide.ttf \
  --unicodes="U+E1B2,U+E1DD,U+E082,U+E0B6,U+E58A,U+E58B,U+E193,U+E070" \
  --output-file=lucide.ttf
```

### Adding an icon

1. Look up the icon's codepoint in `font/info.json` of the same
   `lucide-static` version (`encodedCode`).
2. Add it to the Lucide `--unicodes` list above and re-run the subset from
   the original `lucide.ttf`.
3. Add a variant to `style::Icon` with that codepoint.

### Adding text glyphs

If a character renders through a system fallback font (it will look
slightly different), add its range to `U` and re-subset Inter and
JetBrains Mono.
