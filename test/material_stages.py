"""Stage parity for the Material port: the quantizer, Score and HCT against the venv's materialyoucolor.

Run with the venv's real Python: python3.real -I test/material_stages.py <inir-accel> <scratch dir> <images...>
Pixels are prepared by Pillow exactly as generate_colors_material.py does, so only the quantizer is compared."""
import sys, subprocess, math, random, os, glob
from PIL import Image
from materialyoucolor.quantize import QuantizeCelebi
from materialyoucolor.score.score import Score
from materialyoucolor.hct import Hct
accel, work = sys.argv[1], sys.argv[2]
def prep(path, size=128):
    im = Image.open(path)
    if im.mode in ["L", "P"]: im = im.convert("RGB")
    w, h = im.size
    area = w * h; b = size ** 2
    s = math.sqrt(b / area) if area > b else 1
    nw, nh = max(1, round(w * s)), max(1, round(h * s))
    if nw < w or nh < h: im = im.resize((nw, nh), Image.Resampling.BICUBIC)
    return list(im.get_flattened_data())
ok = bad = 0
files = sys.argv[3:]  # no images: only the HCT and scheme stages
for f in files:
    try: px = prep(f)
    except Exception as e: continue
    raw = os.path.join(work, "px.bin")
    with open(raw, "wb") as fh: fh.write(bytes(v for p in px for v in p[:3]))
    ref = QuantizeCelebi(px, 128); seed = Score.score(ref)[0]
    out = subprocess.run([accel, "__material-probe", "celebi", raw], capture_output=True, text=True).stdout.split("\n")
    got = dict(tuple(map(int, kv.split(":"))) for kv in out[0].split()) if out[0] else {}
    if got == dict(ref) and int(out[1]) == seed: ok += 1
    else:
        bad += 1; print("DIFF", os.path.basename(f), len(ref), len(got), seed, out[1], sorted(set(ref.items()) ^ set(got.items()))[:4])
print("celebi same", ok, "diff", bad)
r = random.Random(11); args = []; trip = []
for _ in range(20000):
    t = (r.uniform(0, 360), r.choice([r.uniform(0, 150), r.uniform(0, 10), 200.0]), r.choice([r.uniform(0, 100), float(r.randint(0, 100))]))
    trip.append(t); args += [repr(x) for x in t]
out = subprocess.run([accel, "__material-probe", "hct"] + args, capture_output=True, text=True).stdout.split("\n")
bad = 0
for t, line in zip(trip, out):
    h = Hct.from_hct(*t); want = f"{h.to_int() & 0xffffffff} {h.hue!r} {h.chroma!r} {h.tone!r}"
    if line != want:
        bad += 1
        if bad < 4: print("HCT", t, line, "want", want)
print("hct", len(trip), "diff", bad)
# Schemes: the nine variants, light and dark, on random seeds plus the edges (greys, pure hues, the dislike band
# and the yellows that bend the 2025 ladders), against MaterialDynamicColors in the order the script writes them.
import importlib
from materialyoucolor.dynamiccolor.material_dynamic_colors import MaterialDynamicColors as MDC
NAMES = [k for k in vars(MDC) if hasattr(getattr(MDC, k), "get_hct")]
TONES = [0, 4, 10, 20, 25, 30, 40, 50, 60, 70, 80, 90, 95, 98, 99, 100]
VARIANTS = {"scheme-fruit-salad": "SchemeFruitSalad", "scheme-expressive": "SchemeExpressive",
            "scheme-monochrome": "SchemeMonochrome", "scheme-rainbow": "SchemeRainbow",
            "scheme-tonal-spot": "SchemeTonalSpot", "scheme-neutral": "SchemeNeutral",
            "scheme-fidelity": "SchemeFidelity", "scheme-content": "SchemeContent", "scheme-vibrant": "SchemeVibrant"}
r = random.Random(int(os.environ.get("SCHEME_RANDOM", "7")))
seeds = [0xFF000000 | r.getrandbits(24) for _ in range(int(os.environ.get("SCHEME_SEEDS", "150")))]
seeds += [0xFF000000, 0xFFFFFFFF, 0xFF808080, 0xFF7F7F7F, 0xFFFF0000, 0xFF00FF00, 0xFF0000FF, 0xFFFFFF00,
          0xFF00FFFF, 0xFFFF00FF, 0xFF6750A4, 0xFFB3A000, 0xFF8A7A00, 0xFFC8B400, 0xFF5B5300, 0xFF7B6F2A]
seeds += [Hct.from_hct(h, c, t).to_int() & 0xFFFFFFFF for h in (95.0, 100.0, 110.0, 115.0, 120.0, 180.0, 260.0)
          for c, t in ((48.0, 40.0), (20.0, 60.0), (80.0, 80.0))]
total = bad = 0
for vname, cname in VARIANTS.items():
    mod = importlib.import_module("materialyoucolor.scheme." + vname.replace("-", "_"))
    Scheme = getattr(mod, cname)
    for dark in (False, True):
        out = subprocess.run([accel, "__material-probe", "scheme", vname, "1" if dark else "0"] + [str(x) for x in seeds],
                             capture_output=True, text=True).stdout.splitlines()
        for seed, line in zip(seeds, out + [""] * len(seeds)):
            s = Scheme(Hct.from_int(seed), dark, 0.0)
            cols = " ".join(str(getattr(MDC, k).get_hct(s).to_int() & 0xFFFFFFFF) for k in NAMES)
            pals = " ".join(str(p.get_hct(t).to_int() & 0xFFFFFFFF) for p in (s.primary_palette, s.secondary_palette,
                            s.tertiary_palette, s.neutral_palette, s.neutral_variant_palette, s.error_palette) for t in TONES)
            want = cols + " | " + pals
            total += 1
            if line != want:
                bad += 1
                if bad < 6:
                    g, w = line.replace("|", "").split(), want.replace("|", "").split()
                    keys = NAMES + [f"pal{i // 16}@{TONES[i % 16]}" for i in range(96)]
                    diff = [(keys[i], hex(int(w[i])), hex(int(g[i])) if i < len(g) else None) for i in range(len(w)) if i >= len(g) or g[i] != w[i]]
                    print("SCHEME", vname, "dark" if dark else "light", hex(seed), len(diff), diff[:5])
print("scheme", total, "diff", bad)
