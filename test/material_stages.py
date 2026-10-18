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
files = sys.argv[3:]
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
