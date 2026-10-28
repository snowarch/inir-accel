# Warm neutrals (opt-in)

`inir-accel washi` gives the same bytes as the Go solver inir had until 3.0 (`scripts/colors/washi`, Version 32); `test/parity.py`
holds the two together. The warm stone/sand look an earlier version baked into every palette is now a separate,
visible option, off by default.

## What it changes

Only the quiet neutrals, never an accent:

| Role | Go solver | Warm neutrals |
| :--- | :--- | :--- |
| `textSecondary`, `textTertiary` | ink hue, chroma `ink * 1.5`, from 62 % / 45 % of the way to the ink | hue turned toward sand (`SUMI_HUE = 80`) by up to `42 * w`, chroma up to `0.024`, from 64 % / 46 % |
| apps `onSurfaceVariant`, `outline` | paper hue, chroma `min(0.02, paper * 1.5 + 0.006)` | turned toward sand by up to `45 * w`, chroma up to `0.028` / `0.022`, from 64 % / 42 % |

`w = warmth / 100 * colour` for the scheme (`tune.<scheme>.warmth`, else the scheme's own warmth, and the Colour
slider). Below `w = 0.01` nothing moves. AIRISU keeps its own ink (`fixedInk`), so its text never leans.

## Turning it on

- `~/.config/inir-accel/options.json`: `{"warmNeutrals": true}`
- or the environment of the shell: `INIR_ACCEL_WARM_NEUTRALS=1` (it wins over the file; `0` turns it off)
- or one call: `inir-accel washi --warm-neutrals --request '<json>'`

The shell re-solves its palette when the request or the version changes, not when this option does: after flipping
it, change any iRiS colour setting once (or delete `~/.local/state/quickshell/user/generated/iris-washi.json`).

Material colours (`generate_colors_material.py`) have no warm option: until the Material port matches the original,
every call runs the original Python.
