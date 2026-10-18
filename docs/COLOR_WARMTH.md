# Calidez de color (warm stone/sand) en `inir-accel`

Este documento explica la divergencia intencional de `inir-accel` respecto de los
generadores originales de `~/inir/scripts/colors` (Go `washi` v30 y
`generate_colors_material.py`) en lo que respecta a la **calidez de los neutros**.

## Objetivo

Que toda la shell y las apps tengan el aspecto cálido "arena / piedra" que ya
tenían Kitty y btop, **sin** perder la fidelidad al wallpaper.

Regla de oro:

| Familia de roles | Quién la decide |
| :--- | :--- |
| `primary`, `secondary`, `tertiary`, accent, highlight, `term1..6`, `term9..14` | **El wallpaper** + Language / Variant / Accent / Highlight / Colour del usuario. Nunca se fuerza un tono. |
| `surface*`, `onSurfaceVariant`, `textSecondary`, `textTertiary`, `outline`, `term7`, `app_subtext` | **Los neutros**: llevan un matiz arena (`SUMI_HUE = 80°`) proporcional a `tune.<scheme>.warmth`. |

## Dónde vive en el código

- [`washi.rs`](../crates/inir-core/src/washi.rs)
  - `SUMI_HUE = 80.0` (fibra de papel washi / sumi).
  - `warmth_of(name, tune)`: usa `tune.<scheme>.warmth` si existe; si no, la calidez propia del esquema.
  - `Solver::apps` y `build_palette`: `warmth_factor = (warmth / 100) * colour`.
    - `onSurfaceVariant` / `outline`: tono desplazado hacia `SUMI_HUE` hasta `45° * warmth_factor`, croma tope `0.028` / `0.022`.
    - `textSecondary` / `textTertiary`: desplazamiento `42° * warmth_factor`, croma tope `0.024`.
    - `secondary` solo recibe un desplazamiento leve (`16° * warmth_factor`); su tono base sigue siendo la semilla del wallpaper.
  - `seed_of("wallpaperHighlight" | "themeHighlight")` usa `best_seed_lch` sobre las semillas reales (tertiary → secondary → wallpaper → primary). **No** hay override amarillo.
  - Airisu (`LangKind::Airisu`) y lenguajes con `fixed_ink` quedan excluidos: tienen paleta propia.
- [`material.rs`](../crates/inir-core/src/material.rs)
  - `generate_material_colors`: rotaciones de `secondary` (+16° / +24° / +45°) y `tertiary` (+48° / −60°) según variante, igual que el pipeline Python. Se pasa `warmth: Some(32.0)` al solver Washi.
  - `run_material_pipeline`: `term7` (`#b0a99f` en `scheme-base.json`) se trata aparte, igual que `generate_colors_material.py`:
    armonía suave con el wallpaper (`harmonize_threshold * 0.35 * harmony`) y croma `0.022..0.042`.
    El resto de `termN` mantiene el clamp `0.08..0.16`.
- [`themegens.rs`](../crates/inir-core/src/themegens.rs) y [`region.rs`](../crates/inir-core/src/region.rs): consumen `term7` / `on_surface_variant` como texto atenuado (btop `inactive_fg`, Kitty `color7`, VS Code, Zed, OpenCode, Vesktop, etc.).

## Bug corregido (2026-10-17)

Una primera pasada fijaba `secondary`, `tertiary` y `highlight` en un amarillo
arena de `82°` con croma `0.076..0.11`, y `term7` heredaba el clamp `0.08` de los
colores de terminal. Resultado: gráficos de btop, botones secundarios, acentos y
`color7` de Kitty salían amarillos/dorados (`#CA9E75`, `#F0AE70`, `#CAA271`) con
**cualquier** wallpaper.

Ahora (wallpaper azul/violeta de referencia):

| Rol | Antes | Ahora |
| :--- | :--- | :--- |
| `primary` | — | `#A0CBFE` |
| `secondary` | `#CA9E75` | `#BCC4F4` |
| `tertiary` / highlight | `#F0AE70` | `#D0B9FF` |
| btop `cpu_start` / `temp_start` | amarillo | `#C7B0F5` |
| Kitty `color7` | `#CAA271` | `#AFAA9A` |

## Relación con los ajustes del usuario

- **iRiS** (`Config.options.iris.appearance`): `language`, `variant`, `theme.surface`,
  `accent` / `accentHue`, `highlight` / `highlightHue`, y por esquema
  `tune.<scheme>.{tone, colour, warmth}`.
  - `warmth = 0` → neutros solo teñidos por el wallpaper.
  - `warmth ≈ 20–40` → matiz arena sutil (default).
  - `warmth ≥ 60` → papel washi marcado.
- **Material** (`Config.options.appearance.wallpaperTheming`): `colorStrength`,
  `softenColors`, `colorInvert` y `terminalColorAdjustments`
  (`saturation`, `brightness`, `harmony`, `backgroundBrightness`) llegan vía
  `switchwall.sh` → `inir-accel material-colors` y se respetan tal cual.
  `colorStrength` y `saturation` también escalan la croma de `term7`.

## Verificación

```bash
cargo test --workspace
cargo build --release -p inir-accel && ./scripts/activate-accelerators.sh install
~/inir/scripts/colors/switchwall.sh --noswitch
REQ=$(jq -r '.request' ~/.local/state/quickshell/user/generated/iris-washi.json)
~/.local/bin/inir-accel washi --all --request "$REQ" \
  --write ~/.local/state/quickshell/user/generated/iris-washi.json
~/inir/scripts/colors/applycolor.sh --force

G=~/.local/state/quickshell/user/generated
jq -r .generated_by $G/theme-meta.json        # → inir-accel (rust)
jq -c '{primary,secondary,tertiary,on_surface_variant}' $G/colors.json
grep -E '^color7 ' ~/.config/kitty/current-theme.conf
grep -E 'inactive_fg|cpu_start' ~/.config/btop/themes/ii-auto.theme
```

Criterio: `secondary`/`tertiary` deben seguir el tono del wallpaper; `term7`
debe ser un gris piedra cálido de croma baja (nunca dorado).
