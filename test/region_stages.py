#!/usr/bin/env python3
"""Stage by stage, inir-accel's region port against the venv's cv2 and numpy (run with the venv's python3.real -I).

    test/region_stages.py [imread] [screen] [--quick]

imread: every wallpaper and synthetic image, colour and grey, against cv2.imread (size and SHA-1 of the bytes).
screen: the screen image of every mode at several screen sizes, against the script's own _read_screen_image.
"""
import glob
import hashlib
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import cv2
import numpy as np

ROOT = Path(__file__).resolve().parent.parent
HOME = Path.home()
SHELL = Path(os.environ.get("INIR_SHELL", HOME / ".config/quickshell/inir"))
# cargo knows where its builds go (a .cargo/config.toml may move them off the repo)
def cargo_target() -> Path:
    try:
        meta = subprocess.run(["cargo", "metadata", "--format-version", "1", "--no-deps"], cwd=ROOT,
                              capture_output=True, text=True, check=True).stdout
        return Path(json.loads(meta)["target_directory"])
    except (OSError, subprocess.CalledProcessError, ValueError, KeyError):
        return ROOT / "target"
TARGET = Path(os.environ.get("CARGO_TARGET_DIR") or cargo_target())
ACCEL = os.environ.get("ACCEL", str(TARGET / "release/inir-accel"))
WALLPAPERS = Path(os.environ.get("WALLPAPERS", HOME / "Pictures/Wallpapers"))

spec = importlib.util.spec_from_file_location("lbr", SHELL / "scripts/images/least_busy_region.py")
lbr = importlib.util.module_from_spec(spec)
spec.loader.exec_module(lbr)


def synth(out: Path) -> list:
    rng = np.random.default_rng(5)
    names = []

    def save(name, img, params=()):
        cv2.imwrite(str(out / name), img, list(params))
        names.append(out / name)

    noise = rng.integers(0, 256, (333, 517, 3), dtype=np.uint8)
    grad = np.dstack([np.tile(np.linspace(0, 255, 640, dtype=np.uint8), (360, 1))] * 3)
    save("noise.png", noise)
    save("noise-q90.jpg", noise, (cv2.IMWRITE_JPEG_QUALITY, 90))
    save("grad.png", grad)
    save("grad-gray.png", cv2.cvtColor(grad, cv2.COLOR_BGR2GRAY))
    save("noise-alpha.png", np.dstack([noise, rng.integers(0, 256, (333, 517), dtype=np.uint8)]))
    save("tiny.png", noise[:7, :5])
    save("one.png", noise[:1, :1])
    save("noise-16.png", noise.astype(np.uint16) * 257 + 3)
    save("noise.webp", noise, (cv2.IMWRITE_WEBP_QUALITY, 80))
    save("noise-lossless.webp", noise, (cv2.IMWRITE_WEBP_QUALITY, 101))
    save("big.jpg", cv2.resize(noise, (3840, 2160), interpolation=cv2.INTER_NEAREST), (cv2.IMWRITE_JPEG_QUALITY, 85))
    save("half.png", cv2.resize(noise, (3840, 2160), interpolation=cv2.INTER_NEAREST))
    save("progressive.jpg", noise, (cv2.IMWRITE_JPEG_QUALITY, 75, cv2.IMWRITE_JPEG_PROGRESSIVE, 1))
    save("interlaced.png", noise)  # cv2 cannot write Adam7; Pillow can
    from PIL import Image
    Image.fromarray(noise[..., ::-1]).save(out / "interlaced.png", interlace=1)
    Image.fromarray(noise[..., ::-1]).convert("P", palette=Image.Palette.ADAPTIVE, colors=16).save(out / "palette.png")
    names.append(out / "palette.png")
    im = Image.fromarray(noise[..., ::-1])
    im.save(out / "gamma.png", gamma=0.45455 if False else None)
    for o in (3, 6, 8):
        ex = Image.Exif()
        ex[0x0112] = o
        im.save(out / f"orient{o}.jpg", exif=ex.tobytes(), quality=90)
        names.append(out / f"orient{o}.jpg")
    with open(out / "truncated.png", "wb") as f:
        f.write((out / "noise.png").read_bytes()[:5000])
    names.append(out / "truncated.png")
    (out / "empty.png").write_bytes(b"")
    names.append(out / "empty.png")
    names.append(out / "missing.png")
    return names


def wallpapers(quick: bool) -> list:
    if not WALLPAPERS.is_dir():
        return []
    files = sorted(p for p in WALLPAPERS.iterdir() if p.suffix.lower() in {".jpg", ".jpeg", ".png", ".webp"} and p.is_file())
    return files[::10] if quick else files


def probe(*args) -> str:
    r = subprocess.run([ACCEL, "__region-probe", *map(str, args)], capture_output=True, text=True)
    return r.stdout.strip() if r.returncode == 0 else f"rc={r.returncode} {r.stderr.strip()[-200:]}"


def describe(img) -> str:
    if img is None:
        return "none"
    img = np.ascontiguousarray(img)
    cn = 1 if img.ndim == 2 else img.shape[2]
    return f"{img.shape[1]} {img.shape[0]} {cn} {hashlib.sha1(img.tobytes()).hexdigest()}"


def main():
    args = sys.argv[1:]
    quick = "--quick" in args
    stages = [a for a in args if a in ("imread", "screen")] or ["imread", "screen"]
    work = Path(tempfile.mkdtemp(prefix="region-stages-"))
    images = synth(work) + wallpapers(quick)
    failed = 0
    with ThreadPoolExecutor(max(2, (os.cpu_count() or 4) - 2)) as ex:
        if "imread" in stages:
            cases = [(p, g) for p in images for g in (0, 1)]

            def one(c):
                p, g = c
                want = describe(cv2.imread(str(p), cv2.IMREAD_GRAYSCALE if g else cv2.IMREAD_COLOR))
                return c, want, probe("imread", p, g)
            bad = [(c, w, got) for c, w, got in ex.map(one, cases) if w != got]
            print(f"imread: {len(cases) - len(bad)} of {len(cases)} identical")
            for (p, g), w, got in bad[:10]:
                print(f"  {p.name} gray={g}: want {w} got {got}")
            failed += len(bad)
        if "screen" in stages:
            sizes = [(1920, 1080), (2560, 1440), (1366, 768), (3840, 2160), (1080, 1920), (800, 600)]
            modes = ["fill", "fit", "stretch", "center", "tile"]
            cases = [(p, g, sizes[i % len(sizes)], m) for i, p in enumerate(images) for g in (0, 1) for m in modes if p.exists() and p.stat().st_size]

            def one(c):
                p, g, (sw, sh), m = c
                want = describe(lbr._read_screen_image(str(p), cv2.IMREAD_GRAYSCALE if g else cv2.IMREAD_COLOR, sw, sh, m))
                return c, want, probe("screen", p, g, sw, sh, m)
            bad = [(c, w, got) for c, w, got in ex.map(one, cases) if w != got]
            print(f"screen: {len(cases) - len(bad)} of {len(cases)} identical")
            for (p, g, s, m), w, got in bad[:10]:
                print(f"  {p.name} gray={g} {s} {m}: want {w} got {got}")
            failed += len(bad)
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
