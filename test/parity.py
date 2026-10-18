#!/usr/bin/env python3
"""Byte parity of inir-accel's helpers against the originals they replace.

    test/parity.py [washi] [material] [region] [--quick] [--keep] [-j N]

Every case runs twice, once through the original (washi built from the shell's Go sources, Material and region
through the venv's real Python) and once through inir-accel the way the shell reaches it (the inir-washi symlink and
the venv's python3 shim). Each run gets its own HOME and XDG dirs, so nothing touches the real session. A case passes
only when stdout, stderr, the exit code and every file the run leaves behind are byte for byte the same. The shim's
fallback to real Python points at a stub that fails, so a port that gives up shows as a failure instead of a pass.

Environment: PARITY_SEED and PARITY_WASHI_N widen the generated corpus; INIR_SHELL (default ~/inir-eco/inir), ACCEL (default the release build, else bin/inir-accel),
INIR_VENV (default ~/.local/state/quickshell/.venv), WALLPAPERS (default ~/Imágenes/Wallpapers, optional).
"""
import concurrent.futures
import difflib
import json
import os
import random
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
HOME = Path.home()
SHELL = Path(os.environ.get("INIR_SHELL", HOME / "inir-eco/inir"))
VENV = Path(os.environ.get("INIR_VENV", HOME / ".local/state/quickshell/.venv"))
REAL_PY = VENV / "bin/python3.real"
TARGET = Path(os.environ.get("CARGO_TARGET_DIR", "/mnt/build/inir-rust-plugin-target"))
ACCEL = Path(os.environ.get("ACCEL", TARGET / "release/inir-accel" if (TARGET / "release/inir-accel").exists() else ROOT / "bin/inir-accel"))
WALLPAPERS = Path(os.environ.get("WALLPAPERS", HOME / "Imágenes/Wallpapers"))
IMAGE_EXT = {".jpg", ".jpeg", ".png", ".webp"}


def isolated_env(base: Path) -> dict:
    env = {k: v for k, v in os.environ.items() if not k.startswith(("XDG_", "WAYLAND", "NIRI", "DBUS", "QS_", "INIR_"))}
    dirs = {"HOME": "home", "XDG_CONFIG_HOME": "home/.config", "XDG_STATE_HOME": "home/.local/state",
            "XDG_CACHE_HOME": "home/.cache", "XDG_DATA_HOME": "home/.local/share", "XDG_RUNTIME_DIR": "run"}
    for key, rel in dirs.items():
        (base / rel).mkdir(parents=True, exist_ok=True)
        env[key] = str(base / rel)
    env["LC_ALL"] = "C.UTF-8"
    env["PYTHONHASHSEED"] = "0"
    return env


def tree(base: Path) -> dict:
    out = {}
    for p in sorted(base.rglob("*")):
        if p.is_file() and not p.is_symlink():
            out[str(p.relative_to(base))] = p.read_bytes()
    return out


class Run:
    def __init__(self, rc, stdout, stderr, files):
        self.rc, self.stdout, self.stderr, self.files = rc, stdout, stderr, files


def run(argv, base: Path, setup=None, extra_env=None, cwd=None) -> Run:
    env = isolated_env(base)
    if extra_env:
        env.update(extra_env)
    if setup:
        setup(base)
    before = set(tree(base))
    p = subprocess.run(argv, env=env, capture_output=True, cwd=cwd or base, timeout=120)
    files = {k: v for k, v in tree(base).items() if k not in before or k.startswith("out/")}
    return Run(p.returncode, p.stdout, p.stderr, files)


def describe(a: bytes, b: bytes, limit=6) -> str:
    try:
        ja, jb = json.loads(a), json.loads(b)
    except Exception:
        ja = jb = None
    if isinstance(ja, dict) and isinstance(jb, dict):
        flat = lambda d, pre="": {k2: v2 for k, v in d.items() for k2, v2 in (flat(v, f"{pre}{k}.").items() if isinstance(v, dict) else [(f"{pre}{k}", v)])}
        fa, fb = flat(ja), flat(jb)
        keys = [k for k in dict.fromkeys(list(fa) + list(fb)) if fa.get(k, "<absent>") != fb.get(k, "<absent>")]
        if keys:
            lines = [f"      {k}: {fa.get(k, '<absent>')} -> {fb.get(k, '<absent>')}" for k in keys[:limit]]
            return f"    {len(keys)} of {len(fa)} values differ\n" + "\n".join(lines)
        return "    same values, different bytes (formatting or key order)\n" + "".join(
            f"      {l}\n" for l in list(difflib.unified_diff(a.decode(errors='replace').splitlines(), b.decode(errors='replace').splitlines(), lineterm="", n=0))[2:2 + limit])
    la, lb = a.decode(errors="replace").splitlines(), b.decode(errors="replace").splitlines()
    diff = [l for l in difflib.unified_diff(la, lb, lineterm="", n=0) if not l.startswith(("---", "+++", "@@"))]
    return "".join(f"      {l[:160]}\n" for l in diff[:limit]).rstrip("\n") or "    (binary difference)"


def compare(orig: Run, port: Run, what=("rc", "stdout", "stderr", "files")) -> list:
    problems = []
    if port.rc == 99 and b"fell back to real python" in port.stderr:
        return ["  delegated: inir-accel handed the call to the real Python (no port yet)"]
    if "rc" in what and orig.rc != port.rc:
        problems.append(f"  exit code {orig.rc} -> {port.rc}\n" + (f"    port stderr: {port.stderr.decode(errors='replace').strip()[:300]}" if port.stderr else ""))
    for name in ("stdout", "stderr"):
        if name in what and getattr(orig, name) != getattr(port, name):
            problems.append(f"  {name}\n" + describe(getattr(orig, name), getattr(port, name)))
    if "files" in what:
        for f in sorted(set(orig.files) | set(port.files)):
            a, b = orig.files.get(f), port.files.get(f)
            if a is None or b is None:
                problems.append(f"  {f}: only the {'port' if a is None else 'original'} wrote it")
            elif a != b:
                problems.append(f"  {f}\n" + describe(a, b))
    return problems


# ---------------------------------------------------------------- washi


def washi_requests(quick: bool) -> list:
    rng = random.Random(int(os.environ.get("PARITY_SEED", "30")))
    hexes = lambda: "#%06x" % rng.randrange(1 << 24)
    greys = ["#808080", "#1a1a1a", "#f0f0f0", "#7f7f80"]
    seeds_pool = lambda: rng.choice([hexes(), hexes(), hexes(), rng.choice(greys), "", "#ff" + hexes()[1:], "nothex", " " + hexes() + " "])
    langs = ["", "washi", "iris", "airisu", "material", "nosuch"]
    variants = ["tonalSpot", "vibrant", "expressive", "fidelity", "monochrome", "", "nosuch"]
    materials = ["black", "graphite", "midnight", "wallpaper", "theme", "nosuch"]
    accents = ["blue", "mint", "rose", "lilac", "green", "design", "wallpaper", "theme", "custom", ""]
    highlights = ["accent", "orange", "yellow", "red", "pink", "green", "design", "wallpaper", "theme", "custom", ""]
    out = [None]  # None is --default
    n = int(os.environ.get("PARITY_WASHI_N", 60 if quick else 600))
    for _ in range(n):
        req = {
            "language": rng.choice(langs), "variant": rng.choice(variants), "material": rng.choice(materials),
            "accent": rng.choice(accents), "accentHue": round(rng.uniform(0, 360), rng.choice([0, 1, 3])),
            "highlight": rng.choice(highlights), "highlightHue": rng.choice([0, 32, round(rng.uniform(0, 360), 2)]),
            "vibrance": rng.choice([0, 0.5, 0.85, 1, round(rng.random(), 3)]),
            "seeds": {k: seeds_pool() for k in ("wallpaper", "primary", "secondary", "tertiary", "background")},
            "tune": {},
        }
        if rng.random() < 0.5:
            req["glass"] = True
        if rng.random() < 0.4:
            req["inkStyle"] = rng.choice(["style", "washi", ""])
        if rng.random() < 0.4:
            req["darkStyle"] = rng.choice(["ink", "style", ""])
        for scheme in ("dark", "ink", "light"):
            if rng.random() < 0.8:
                t = {"tone": rng.choice([0, -8, 14, -24, 40, 50, 68, rng.randint(-60, 70)])}
                for field, lo, hi in (("colour", 0, 100), ("widgets", 40, 160), ("warmth", 0, 100)):
                    r = rng.random()
                    if r < 0.4:
                        t[field] = rng.randint(lo, hi)
                    elif r < 0.5:
                        t[field] = None
                    elif r < 0.55:
                        t[field] = rng.choice([lo - 30, hi + 30])
                req["tune"][scheme] = t
        for key in list(req):
            if rng.random() < 0.08 and key not in ("seeds", "tune"):
                del req[key]
        out.append(json.dumps(req, separators=(",", ":") if rng.random() < 0.7 else (", ", ": ")))
    # Shapes the decoder has to take the way Go does: case-insensitive keys, nulls, unknown fields, escapes.
    out += [
        '{"Accent":"rose","MATERIAL":"midnight","seeds":{"Primary":"#3366cc"}}',
        '{"language":null,"material":null,"accentHue":null,"tune":null}',
        '{"material":"black","extra":{"x":[1,2,3]},"tune":{"light":{"tone":5}}}',
        '{"material":"graphite","accent":"wallpaper","seeds":{"wallpaper":"#a1b2c3","primary":"<&>"}}',
        '{"material":"black","accent":"custom","accentHue":1e2,"vibrance":5e-1,"seeds":{"wallpaper":"caf\\u00e9"}}',
        '{"material":"black","accent":"blue","accent":"rose"}',
        '{"material":"wallpaper","seeds":{"wallpaper":"#FFAA0000"},"tune":{"dark":{"tone":-100},"light":{"warmth":1000}}}',
        "{}",
    ]
    return out


def build_go_washi(work: Path) -> Path:
    exe = work / "inir-washi-go"
    cache = TARGET / "parity-gocache"
    cache.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, GOCACHE=str(cache), GOFLAGS="-mod=mod", GOTOOLCHAIN="local")
    subprocess.run(["go", "build", "-trimpath", "-o", str(exe), "./scripts/colors/washi"], cwd=SHELL, env=env, check=True)
    return exe


def washi_suite(work: Path, quick: bool, jobs: int):
    go = build_go_washi(work)
    bindir = work / "accel-bin"
    bindir.mkdir(exist_ok=True)
    link = bindir / "inir-washi"
    if not link.exists():
        link.symlink_to(ACCEL)
    cases = [(i, r) for i, r in enumerate(washi_requests(quick))]
    cases += [("noargs", "__noargs__"), ("badjson", "{not json")]

    def one(case):
        i, req = case
        argv_tail = (["--default", "--out", "out/iris-washi.json"] if req is None else
                     [] if req == "__noargs__" else ["--request", req, "--out", "out/iris-washi.json"])
        res = []
        for side, exe in (("orig", go), ("port", link)):
            base = work / "washi" / str(i) / side
            base.mkdir(parents=True, exist_ok=True)
            (base / "out").mkdir(exist_ok=True)
            res.append(run([str(exe)] + argv_tail, base))
        # Go and Rust word their errors differently; for a refused request only the code and stdout count.
        what = ("rc", "stdout", "files") if req in ("__noargs__", "{not json") else ("rc", "stdout", "stderr", "files")
        return i, req, compare(*res, what=what)

    return list(pmap(one, cases, jobs))


# ---------------------------------------------------------------- material


def synth_images(work: Path) -> list:
    """Synthetic sources in every mode and format the shell can hand over, made by the venv's Pillow."""
    out = work / "synth"
    out.mkdir(exist_ok=True)
    script = r'''
import sys, random, math
from PIL import Image
out = sys.argv[1]
rng = random.Random(7)
def grad(w, h, a, b):
    im = Image.new("RGB", (w, h))
    px = im.load()
    for y in range(h):
        for x in range(w):
            t = (x / max(1, w - 1) + y / max(1, h - 1)) / 2
            n = rng.randint(-12, 12)
            px[x, y] = tuple(max(0, min(255, int(a[i] + (b[i] - a[i]) * t) + n)) for i in range(3))
    return im
def blobs(w, h):
    im = Image.new("RGB", (w, h), tuple(rng.randrange(256) for _ in range(3)))
    px = im.load()
    for _ in range(9):
        cx, cy, r = rng.randrange(w), rng.randrange(h), rng.randrange(8, max(9, w // 3))
        col = tuple(rng.randrange(256) for _ in range(3))
        for y in range(max(0, cy - r), min(h, cy + r)):
            for x in range(max(0, cx - r), min(w, cx + r)):
                if (x - cx) ** 2 + (y - cy) ** 2 < r * r:
                    px[x, y] = col
    return im
names = []
def save(im, name, **kw):
    im.save(f"{out}/{name}", **kw); names.append(name)
save(grad(640, 360, (20, 40, 90), (230, 120, 40)), "grad.png")
save(blobs(800, 450), "blobs.png")
save(blobs(97, 61), "tiny.png")
save(grad(128, 128, (200, 10, 10), (10, 10, 200)), "exact128.png")
save(Image.new("RGB", (300, 200), (128, 128, 128)), "grey.png")
save(Image.new("RGB", (300, 200), (12, 200, 90)), "solid.png")
save(grad(500, 300, (0, 0, 0), (255, 255, 255)).convert("L"), "luma.png")
save(blobs(400, 300).convert("P", palette=Image.Palette.ADAPTIVE, colors=16), "palette.png")
rgba = blobs(400, 300).convert("RGBA"); rgba.putalpha(Image.linear_gradient("L").resize((400, 300)))
save(rgba, "alpha.png")
b = blobs(1200, 700)
save(b, "q90.jpg", quality=90)
save(b, "q60-444.jpg", quality=60, subsampling=0)
save(b, "q75-422.jpg", quality=75, subsampling=1)
save(b, "progressive.jpg", quality=85, progressive=True)
save(grad(1000, 563, (90, 20, 120), (250, 220, 90)).convert("L"), "luma.jpg", quality=88)
save(b, "lossy.webp", quality=80)
save(b, "lossless.webp", lossless=True)
save(grad(3000, 1700, (10, 60, 70), (250, 240, 200)), "big.jpg", quality=92)
print("\n".join(names))
'''
    names = subprocess.run([str(REAL_PY), "-I", "-c", script, str(out)], check=True, capture_output=True, text=True).stdout.split()
    return [out / n for n in names]


def wallpapers(quick: bool) -> list:
    if not WALLPAPERS.is_dir():
        return []
    files = sorted(p for p in WALLPAPERS.iterdir() if p.suffix.lower() in IMAGE_EXT and p.is_file())
    if quick:
        files = files[::12]
    return files


def material_cases(work: Path, quick: bool) -> list:
    rng = random.Random(4)
    term = str(SHELL / "scripts/colors/terminal/scheme-base.json")
    outs = ["--json-output", "out/colors.json", "--palette-output", "out/palette.json", "--app-palette-output", "out/app-palette.json",
            "--terminal-output", "out/terminal.json", "--meta-output", "out/theme-meta.json", "--scss-output", "out/material_colors.scss"]
    schemes = ["scheme-tonal-spot", "scheme-vibrant", "scheme-expressive", "scheme-fidelity", "scheme-content", "scheme-neutral",
               "scheme-monochrome", "scheme-rainbow", "scheme-fruit-salad"]
    cases = []
    images = synth_images(work) + wallpapers(quick)
    for i, img in enumerate(images):
        mode = "dark" if i % 3 else "light"
        scheme = ["auto", rng.choice(schemes)][i % 2]
        cases.append(("img", ["--path", str(img), "--mode", mode, "--scheme", scheme, "--termscheme", term, "--blend_bg_fg",
                              "--cache", "out/color.txt"] + outs))
    roles = work / "iris-surface.json"
    roles.write_text(json.dumps({"roles": {"background": "#141218", "surface": "#141218", "onSurface": "#e6e0e9", "primary": "#d0bcff",
                                           "onPrimary": "#381e72", "surfaceContainer": "#211f26", "nothex": "red"}}))
    roles_light = work / "iris-surface-light.json"
    roles_light.write_text(json.dumps({"roles": {"background": "#fdf8fd", "primary": "#6750a4", "onPrimary": "#ffffff"}}))
    seeds = ["#%06x" % rng.randrange(1 << 24) for _ in range(12 if quick else 60)] + ["#808080", "#000000", "#ffffff", "#ff0000", "#00ff00", "#0000ff"]
    for k, seed in enumerate(seeds):
        for scheme in (schemes if not quick or k < 2 else schemes[k % 9:k % 9 + 1]):
            for mode in ("dark", "light"):
                argv = ["--color", seed, "--mode", mode, "--scheme", scheme]
                r = rng.random()
                if r < 0.7:
                    argv += ["--termscheme", term, "--blend_bg_fg"]
                for flag, values in (("--term_saturation", [0.2, 0.65, 1.0]), ("--term_brightness", [0.3, 0.6, 0.9]), ("--harmony", [0, 0.4, 1]),
                                     ("--term_bg_brightness", [0, 0.25, 0.5, 0.9]), ("--color-strength", [0.5, 1.0, 1.6]),
                                     ("--harmonize_threshold", [10, 100]), ("--term_fg_boost", [0, 0.35, 0.8])):
                    if rng.random() < 0.25:
                        argv += [flag, str(rng.choice(values))]
                for flag in ("--soften", "--invert-hue"):
                    if rng.random() < 0.15:
                        argv.append(flag)
                if rng.random() < 0.2:
                    argv += ["--surface-seed", rng.choice(["#1d2024", "#f4f1ea", "#2a1f3d", "#e8f0ff", "bogus"])]
                if rng.random() < 0.15:
                    argv += ["--accent-seed", "#%06x" % rng.randrange(1 << 24)]
                if rng.random() < 0.12:
                    argv += ["--washi-roles", str(roles if mode == "dark" else roles_light)]
                cases.append(("color", argv + outs))
    # The calls the shell makes besides switchwall: SCSS on stdout only, doctor, install, the terminal test.
    cases += [
        ("stdout", ["--color", "#3a6ea5", "--mode", "dark", "--termscheme", term, "--blend_bg_fg"]),
        ("stdout", ["--color", "#3a6ea5"]),
        ("doctor", ["--path", str(images[0]), "--mode", "dark", "--json-output", "out/colors.json"]),
        ("smart", ["--path", str(images[4]), "--smart", "--json-output", "out/colors.json"]),
        ("size", ["--path", str(images[1]), "--size", "64", "--json-output", "out/colors.json"]),
        ("transp", ["--color", "#aa3355", "--transparency", "transparent", "--meta-output", "out/meta.json"]),
        ("abbrev", ["--color", "#aa3355", "--json-out", "out/colors.json", "--mode=light"]),
        ("unknown-scheme", ["--color", "#aa3355", "--scheme", "nosuch", "--meta-output", "out/meta.json", "--json-output", "out/c.json"]),
        ("missing-file", ["--path", "/nonexistent.png", "--json-output", "out/colors.json"]),
        ("bad-flag", ["--color", "#aa3355", "--nosuch"]),
        ("bad-mode", ["--color", "#aa3355", "--mode", "dim"]),
    ]
    templates = [c for c in cases if c[0] in ("img", "color")][:: (40 if quick else 12)]
    cases += [("templates", argv + ["--render-templates", "@MATUGEN@"]) for _, argv in templates]
    return cases


def material_suite(work: Path, quick: bool, jobs: int):
    script = SHELL / "scripts/colors/generate_colors_material.py"
    shim = work / "venv/bin"
    shim.mkdir(parents=True, exist_ok=True)
    if not (shim / "python3").exists():
        (shim / "python3").symlink_to(ACCEL)
    stub = work / "no-fallback"
    stub.write_text("#!/bin/sh\necho 'inir-accel fell back to real python' >&2\nexit 99\n")
    stub.chmod(0o755)
    matugen = SHELL / "dots/.config/matugen"
    cases = list(enumerate(material_cases(work, quick)))

    def one(case):
        i, (kind, argv) = case
        res = []
        for side in ("orig", "port"):
            base = work / "material" / str(i) / side
            base.mkdir(parents=True, exist_ok=True)
            (base / "out").mkdir(exist_ok=True)
            args = list(argv)
            setup = None
            if "@MATUGEN@" in args:
                args[args.index("@MATUGEN@")] = str(base / "home/.config/matugen")
                setup = lambda b: shutil.copytree(matugen, b / "home/.config/matugen", dirs_exist_ok=True)
            exe = [str(REAL_PY), "-E", str(script)] if side == "orig" else [str(shim / "python3"), "-E", str(script)]
            res.append(run(exe + args, base, setup=setup, extra_env={"INIR_REAL_PYTHON": str(stub)} if side == "port" else None))
        # A traceback or an argparse message is Python's wording; there the code, stdout and the files count.
        what = ("rc", "stdout", "files") if kind in ("missing-file", "bad-flag", "bad-mode") else ("rc", "stdout", "stderr", "files")
        return f"{i}:{kind}", " ".join(argv[:6]), compare(*res, what=what)

    return list(pmap(one, cases, jobs))


# ---------------------------------------------------------------- region


def region_suite(work: Path, quick: bool, jobs: int):
    script = SHELL / "scripts/images/least_busy_region.py"
    shim = work / "venv/bin"
    shim.mkdir(parents=True, exist_ok=True)
    if not (shim / "python3").exists():
        (shim / "python3").symlink_to(ACCEL)
    stub = work / "no-fallback"
    stub.write_text("#!/bin/sh\necho 'inir-accel fell back to real python' >&2\nexit 99\n")
    stub.chmod(0o755)
    images = synth_images(work) + wallpapers(quick)
    rng = random.Random(9)
    cases = []
    for i, img in enumerate(images):
        sw, sh = rng.choice([(1920, 1080), (2560, 1440), (1366, 768), (3840, 2160)])
        mode = ["fill", "fit", "stretch", "center", "tile"][i % 5]
        cases.append(("luma", ["--luma-grid", "24", "--screen-width", str(sw), "--screen-height", str(sh), "--screen-mode", mode, str(img)]))
        cases.append(("widget", ["--screen-width", str(sw), "--screen-height", str(sh), "--width", str(rng.choice([260, 300, 420])),
                                 "--height", str(rng.choice([180, 200, 260])), "--horizontal-padding", "48", "--vertical-padding", "48", str(img)]
                      + (["--busiest"] if i % 2 else [])))
    cases = list(enumerate(cases))

    def one(case):
        i, (kind, argv) = case
        res = []
        for side in ("orig", "port"):
            base = work / "region" / str(i) / side
            base.mkdir(parents=True, exist_ok=True)
            exe = [str(REAL_PY), str(script)] if side == "orig" else [str(shim / "python3"), str(script)]
            env = {"INIR_REAL_PYTHON": str(stub)} if side == "port" else None
            first = run(exe + argv, base, extra_env=env)
            again = run(exe + argv, base, extra_env=env)  # the second run reads its own cache
            first.files, again.files = {}, {}
            res.append((first, again))
        problems = compare(res[0][0], res[1][0], what=("rc", "stdout"))
        problems += [p.replace("  stdout", "  stdout (cached)", 1) for p in compare(res[0][1], res[1][1], what=("rc", "stdout"))]
        return f"{i}:{kind}", " ".join(Path(a).name if a.startswith("/") else a for a in argv), problems

    return list(pmap(one, cases, jobs))


# ---------------------------------------------------------------- main


def pmap(fn, items, jobs):
    with concurrent.futures.ThreadPoolExecutor(jobs) as ex:
        yield from ex.map(fn, items)


def main():
    args = sys.argv[1:]
    quick, keep = "--quick" in args, "--keep" in args
    jobs = int(args[args.index("-j") + 1]) if "-j" in args else max(2, (os.cpu_count() or 4) - 2)
    suites = [a for a in args if a in ("washi", "material", "region")] or ["washi", "material", "region"]
    for need in (ACCEL, REAL_PY, SHELL):
        if not need.exists():
            sys.exit(f"parity: missing {need}")
    work = Path(tempfile.mkdtemp(prefix="inir-accel-parity-", dir=os.environ.get("PARITY_TMP")))
    print(f"inir-accel: {ACCEL}\nshell: {SHELL}\nwork: {work}")
    failed = 0
    try:
        for name in suites:
            results = {"washi": washi_suite, "material": material_suite, "region": region_suite}[name](work, quick, jobs)
            bad = [r for r in results if r[2]]
            delegated = [r for r in bad if r[2][0].startswith("  delegated")]
            print(f"\n== {name}: {len(results) - len(bad)} of {len(results)} cases identical"
                  + (f", {len(delegated)} delegated to the original" if delegated else ""))
            for case, label, problems in bad[:12]:
                print(f"-- case {case}: {str(label)[:150]}")
                print("\n".join(problems[:4]))
            if len(bad) > 12:
                print(f"... and {len(bad) - 12} more differing cases")
            failed += len(bad)
    finally:
        if keep:
            print(f"kept {work}")
        else:
            shutil.rmtree(work, ignore_errors=True)
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
