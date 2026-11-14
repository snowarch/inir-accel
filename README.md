# inir-accel

What [iNiR](https://github.com/snowarch/iNiR) runs in the background, in Rust: wallpaper colours, the busy-region
sampler behind desktop widgets, thumbnails, sensors, app search, YouTube Music, lyrics, keybinds and most of what used
to be a Python or Go helper. Same output, byte for byte, a lot less waiting.

It sits next to the other pieces of iNiR and does not step on them:

- [inir-cli](https://github.com/snowarch/inir-cli) is what you type (`inir update`, `inir doctor`, `inir hub`).
- [qs-rs](https://github.com/snowarch/qs-rs) runs the shell itself.
- [inir-hub](https://github.com/snowarch/inir-hub) holds the families, themes and widgets you install
  ([inir.space/hub](https://inir.space/hub)).
- inir-accel is what the shell runs in the background. It never edits the shell's files.

## What's inside

- `crates/inir-core`: the work.
  - `washi.rs`, `material.rs`: the washi solver (Version 32: `washi`, `iris`, `airisu`, `material:*`) and Material
    colours from a wallpaper (HCT, the quantizer, all 9 schemes, terminal colours and templates), ported to the byte,
    with Go's float maths and JSON in `gomath.rs` and `gojson.rs`.
  - `region.rs`, `findregions.rs`, `thumbgen.rs`: the screen fits and `--luma-grid` the widgets read (cached in
    `~/.cache/inir/region-sampling/`), image regions, freedesktop thumbnails in parallel.
  - `terminals.rs`, `vscode.rs`, `zed.rs`, `opencode.rs`, `system24.rs`, `claude_theme.rs`: the theme generators.
  - `innertube.rs`, `ytrate.rs`, `lyrics.rs`, `study.rs`, `voice.rs`, `aimodels.rs`, `discord_compress.rs`: YouTube
    Music, lyrics, study decks, voice search, AI model catalogs, Discord compression.
  - `sensors.rs`, `apps.rs`, `prewarm.rs`, `keybinds.rs`, `osk.rs`, `daemons.rs`, `sddm.rs`, `niriconfig/`: hwmon and GPU
    discovery, `.desktop` scan and fuzzy search, the QML VFS qs-rs loads, keybinds, OSK labels, the small daemons, SDDM
    sync, the niri config Settings edits.
- `crates/inir-accel`: one multi-call binary (`inir-accel help` lists every subcommand). The shell runs it through
  `scripts/lib/accel-run`, and it stands in for `inir-washi` and the VSCode, Zed and OpenCode theme generators under
  those names.
- `crates/inir-qml-plugin`: `import Inir.Native 1.0`, a cxx-qt singleton with the same work as QML calls, no
  process spawned.

## Numbers

inir-accel 0.1.4, 2026-11-14, on a Ryzen 7 5700G with Linux 7.3, each run from an empty cache:

| What the shell does | Input | Time |
| :--- | :--- | ---: |
| widget legibility grid | a 3840x2160 JPEG filling a 1920x1080 screen | 213 ms, 15 ms cached |
| Material colours (palettes, terminal, 6 templates) | the same wallpaper | 183 ms |
| thumbnails, 4 workers as the shell runs them | 350 wallpapers up to 10240 px wide (188 JPEG, 159 PNG, 3 WebP) | 7.2 s |
| washi, a fresh install's palette | | 58 ms (the Go build inir had until 3.0: 59 ms, same bytes) |
| sensors (hwmon and GPU) | | 0.76 ms |

A JPEG is decoded at the smallest DCT step that still covers its thumbnail: the 188 JPEGs above take 3.8 s, 5.5 s on
0.1.3. What is left is reading the file itself (a 20 MB JPEG takes about 250 ms alone).

## Using it

iNiR's setup and update install it prebuilt for x86_64 and aarch64 ([Releases](https://github.com/snowarch/inir-accel/releases),
built by CI on every `v*` tag and pinned by sha256 in iNiR's base). To build it yourself you need Rust 1.85 or newer
(edition 2024) and, for the QML plugin, Qt 6 with its development files (cxx-qt finds it through `QMAKE`, or
`qmake6`/`qmake` on `PATH`).

```sh
cargo build --release
scripts/activate-accelerators.sh install     # hooks into an installed iNiR
scripts/activate-accelerators.sh status
bin/inir-accel bench
scripts/activate-accelerators.sh uninstall   # puts the original python3 and Go helpers back
```

`install` copies the binary to `bin/inir-accel` (so cleaning `target/` doesn't unhook anything), links it at
`~/.local/bin/inir-accel`, and points those helper names at it, keeping any originals beside them (`*.go.bak`, and
`python3.real` on an install that still has the v2 venv). Everything it touches lives in `~/.local/state/quickshell/`,
`~/.local/share/inir/accel/` and that one link, so `uninstall` leaves the shell exactly as it was. For `Inir.Native` in your own QML, `scripts/activate-accelerators.sh env` prints the
`PATH` and `QML2_IMPORT_PATH` lines.

Colours match the original by default. If you want the quiet greys (secondary text, outlines) to lean sandy with the
Warmth slider, set `{"warmNeutrals": true}` in `~/.config/inir-accel/options.json` or `INIR_ACCEL_WARM_NEUTRALS=1`.
More in [docs/COLOR_WARMTH.md](docs/COLOR_WARMTH.md).

## Parity

`test/parity.py` runs every case twice, once through the original (the Go washi and theme generators built from the
last iNiR commit that had them, Python ports through a v2.33 venv's real Python) and once through inir-accel the way the shell reaches it, each in its own HOME and XDG dirs, and compares stdout, stderr,
exit code and every file written. A port that gives up and falls back to Python counts as delegated, not equal.

```sh
python3 test/parity.py            # all 12: washi, material, region, findregions, the theme generators, niriconfig...
python3 test/parity.py washi      # one of them; --quick for a smaller corpus
PARITY_SEED=2 PARITY_WASHI_N=3000 python3 test/parity.py washi
```

`INIR_SHELL` points it at a shell checkout (default: the installed one, `~/.config/quickshell/inir`). The originals left
the shell at 3.0: the Go ones come from its git history, the Python ones need a v2.33 venv (`INIR_VENV`).

## License

LGPL-3.0-only (`LICENSE`, with the GPL text it builds on in `LICENSE-GPL`).

Made by Hikari.
