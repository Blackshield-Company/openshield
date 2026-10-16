# Fonts and licensing

All three brand typefaces are licensed under the **SIL Open Font License 1.1 (OFL)**.

| Family | Designer / project | License | Source |
|---|---|---|---|
| Cinzel | Natanael Gama | OFL 1.1 | https://fonts.google.com/specimen/Cinzel |
| Inter | The Inter Project Authors | OFL 1.1 | https://github.com/rsms/inter |
| JetBrains Mono | The JetBrains Mono Project Authors | OFL 1.1 | https://github.com/JetBrains/JetBrainsMono |

What this means for OpenShield:

- `wordmark.svg` and `lockup.svg` contain the Cinzel glyphs **converted to
  outlines**. Shipping outlined artwork is allowed by the OFL and does not
  require bundling the font or its license.
- These repos do **not** bundle any font files today. Apps reference the
  families by name and fall back to system fonts (see `tokens.css`).
- If a font file is ever bundled (for example in the desktop app), include its
  `OFL.txt` next to it, keep the copyright notice, do not sell the font on its
  own, and do not use the Reserved Font Name for a modified version.
