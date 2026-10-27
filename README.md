# inir-accel

Rust stand-ins for the Python and Go helpers [iNiR](https://github.com/snowarch/iNiR) spawns while you use it:
wallpaper colours, the busy-region sampler behind desktop widgets, thumbnails, sensors, app search. Same output,
byte for byte, a lot less waiting.

It sits next to the other pieces of iNiR and does not step on them:

- [inir-cli](https://github.com/snowarch/inir-cli) is what you type (`inir update`, `inir doctor`, `inir hub`).
- [qs-rs](https://github.com/snowarch/qs-rs) runs the shell itself.
- [inir-hub](https://github.com/snowarch/inir-hub) holds the families, themes and widgets you install
  ([inir.space/hub](https://inir.space/hub)).
- inir-accel is what the shell runs in the background. It never edits the shell's files.

## What's inside

- `crates/inir-core`: the work.
  - `region.rs`: `least_busy_region.py` without OpenCV or NumPy (screen fits, the `--luma-grid` the widgets read,
    a binary cache in `~/.cache/inir/region-sampling/`).
  - `material.rs`: `generate_colors_material.py` ported to the byte (materialyoucolor 3.0.4, Pillow 12.3.0, numpy
    2.5.4: HCT, the quantizer, all 9 schemes, terminal colours and templates). It only answers when the script and
    those versions match exactly; anything else goes to Python.
  - `washi.rs`: the Go washi solver (Version 30: `washi`, `iris`, `airisu`, `material:*`), with Go's float maths in
    `gomath.rs` and Go's JSON in `gojson.rs`, so the bytes agree.
  - `themegens.rs`, `vscode.rs`: the terminal, VSCode (and forks), OpenCode, Zed and system24 theme generators.
  - `thumbgen.rs`: `thumbgen.py` in parallel, same freedesktop thumbnail paths as the shell.
  - `sensors.rs`, `apps.rs`, `prewarm.rs`: hwmon and GPU discovery, `.desktop` scan and fuzzy search, the QML VFS
    qs-rs loads.
- `crates/inir-accel`: one multi-call binary. Run it by subcommand (`inir-accel help` lists them), or let it stand in
  for the shell's venv `python3` and three of its Go helpers (washi, the VSCode and OpenCode theme generators: the
  ones whose output matched byte for byte; Zed and system24 stay Go). As `python3` it answers only the scripts it has
  ported (colours, the region sampler, thumbnails, sensors, terminal configs, a few daemons), and only while each one
  still matches the version it was ported from; anything else goes to the real Python untouched.
- `crates/inir-qml-plugin`: `import Inir.Native 1.0`, a cxx-qt singleton with the same work as QML calls, no
  process spawned.

## Numbers

Measured on a live session, a 4K wallpaper and a folder of 350:

| What the shell does | Python/Go | inir-accel |
| :--- | ---: | ---: |
| widget legibility grid, cold | 501 ms | 126 ms |
| same, cached | 501 ms | 14 ms |
| Material colours from a wallpaper (with terminal and templates) | 872 ms | 198 ms |
| 350 thumbnails | 3.5 to 12 s | 459 ms |
| washi (3 schemes, 7 styles) | 18 to 25 ms | 12.7 ms |
| sensors | 45 ms | 0.78 ms |

## Using it

iNiR doesn't ship it yet, so for now you build it. You need Rust 1.85 or newer (edition 2024) and, for the QML
plugin, Qt 6 with its development files (cxx-qt finds it through `QMAKE`, or `qmake6`/`qmake` on `PATH`).

```sh
cargo build --release
scripts/activate-accelerators.sh install     # hooks into an installed iNiR
scripts/activate-accelerators.sh status
bin/inir-accel bench
scripts/activate-accelerators.sh uninstall   # puts the original python3 and Go helpers back
```

`install` copies the binary to `bin/inir-accel` (so cleaning `target/` doesn't unhook anything), links it at
`~/.local/bin/inir-accel`, and points the venv `python3` and those three Go helpers at it, keeping the originals as
`python3.real` and `*.go.bak`. Everything it touches lives in `~/.local/state/quickshell/`, so `uninstall` leaves the
shell exactly as it was. For `Inir.Native` in your own QML, `scripts/activate-accelerators.sh env` prints the
`PATH` and `QML2_IMPORT_PATH` lines.

Colours match the original by default. If you want the quiet greys (secondary text, outlines) to lean sandy with the
Warmth slider, set `{"warmNeutrals": true}` in `~/.config/inir-accel/options.json` or `INIR_ACCEL_WARM_NEUTRALS=1`.
More in [docs/COLOR_WARMTH.md](docs/COLOR_WARMTH.md).

## Parity

`test/parity.py` runs every case twice, once through the original (the shell's Go washi, the venv's real Python) and
once through inir-accel the way the shell reaches it, each in its own HOME and XDG dirs, and compares stdout, stderr,
exit code and every file written. A port that gives up and falls back to Python counts as delegated, not equal.

```sh
python3 test/parity.py            # washi, material, region, opencode, vscode
python3 test/parity.py washi      # one of them; --quick for a smaller corpus
PARITY_SEED=2 PARITY_WASHI_N=3000 python3 test/parity.py washi
```

`INIR_SHELL` points it at a shell checkout (default: the installed one, `~/.config/quickshell/inir`).

## License

LGPL-3.0-only (`LICENSE`, with the GPL text it builds on in `LICENSE-GPL`).

Made by Hikari.
