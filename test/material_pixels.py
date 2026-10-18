"""Pixel parity for the Material port: Image.open, convert, BICUBIC resize and the auto-scheme statistics.

Run with the venv's real Python: python3.real -I test/material_pixels.py <inir-accel> <scratch dir> [images...]
Without images it uses ~/Imágenes/Wallpapers. Hand-built PNGs cover every colour type and bit depth; the modes the
port leaves to the original (1-bit and 16-bit grey, grey with alpha, GIF) must come back delegated, the rest equal."""
import sys, os, struct, zlib, random, subprocess, math, glob
import numpy as np
from PIL import Image

accel, work = sys.argv[1], sys.argv[2]
files = sys.argv[3:] or sorted(p for p in glob.glob(os.path.expanduser("~/Imágenes/Wallpapers/*"))
                               if p.lower().endswith((".png", ".jpg", ".jpeg", ".webp")))
DELEGATE = 125


def fnv(data: bytes) -> int:
    h = 0xcbf29ce484222325
    for b in data:
        h = ((h ^ b) * 0x100000001b3) & 0xFFFFFFFFFFFFFFFF
    return h


def stats(pil_image):
    # _auto_detect_scheme's statistics, line for line.
    arr = np.array(pil_image, dtype=np.float64)
    R, G, B = arr[:, :, 0], arr[:, :, 1], arr[:, :, 2]
    rg = np.absolute(R - G)
    yb = np.absolute(0.5 * (R + G) - B)
    colorfulness = float(np.sqrt(np.std(rg) ** 2 + np.std(yb) ** 2) + 0.3 * np.sqrt(np.mean(rg) ** 2 + np.mean(yb) ** 2))
    r, g, b = R / 255.0, G / 255.0, B / 255.0
    maxc = np.maximum(np.maximum(r, g), b)
    minc = np.minimum(np.minimum(r, g), b)
    delta = maxc - minc
    h = np.zeros_like(r)
    with np.errstate(invalid="ignore", divide="ignore"):
        s = np.where(maxc > 0, (maxc - minc) / maxc, 0.0) * 255
        mask = delta > 0
        idx = (maxc == r) & mask
        h[idx] = 30.0 * (((g[idx] - b[idx]) / delta[idx]) % 6)
        idx = (maxc == g) & mask
        h[idx] = 30.0 * ((b[idx] - r[idx]) / delta[idx] + 2)
        idx = (maxc == b) & mask
        h[idx] = 30.0 * ((r[idx] - g[idx]) / delta[idx] + 4)
    return colorfulness, float(np.mean(s)), float(np.std(h))


def prepare(path, size):
    image = Image.open(path)
    if image.format == "GIF":
        return None
    if image.mode in ["L", "P"]:
        image = image.convert("RGB")
    w, h = image.size
    area, bitmap = w * h, size ** 2
    scale = math.sqrt(bitmap / area) if area > bitmap else 1
    nw, nh = max(1, round(w * scale)), max(1, round(h * scale))
    if nw < w or nh < h:
        image = image.resize((nw, nh), Image.Resampling.BICUBIC)
    return image


def chunk(kind, body):
    return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)


def raw_png(path, w, h, depth, colour, rows, extra=b""):
    # rows: per row, the packed scanline bytes (no filter byte).
    data = b"".join(b"\x00" + r for r in rows)
    ihdr = struct.pack(">IIBBBBB", w, h, depth, colour, 0, 0, 0)
    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + extra + chunk(b"IDAT", zlib.compress(data)) + chunk(b"IEND", b""))


def pack_bits(values, depth):
    out, acc, n = bytearray(), 0, 0
    for v in values:
        acc = (acc << depth) | v
        n += depth
        if n == 8:
            out.append(acc); acc = n = 0
    if n:
        out.append(acc << (8 - n))
    return bytes(out)


def crafted():
    rng = random.Random(3)
    out = os.path.join(work, "crafted")
    os.makedirs(out, exist_ok=True)
    made = []
    for (w, h) in ((300, 200), (90, 70)):
        for depth in (1, 2, 4, 8, 16):
            mx = (1 << depth) - 1
            px = [[rng.randrange(mx + 1) for _ in range(w)] for _ in range(h)]
            pack = (lambda r: b"".join(struct.pack(">H", v) for v in r)) if depth == 16 else (lambda r: pack_bits(r, depth))
            p = f"{out}/grey{depth}-{w}.png"; raw_png(p, w, h, depth, 0, [pack(r) for r in px]); made.append((p, depth in (1, 16)))
            if depth == 8:
                p = f"{out}/grey8trns-{w}.png"
                raw_png(p, w, h, 8, 0, [pack(r) for r in px], chunk(b"tRNS", struct.pack(">H", px[0][0]))); made.append((p, False))
            if depth <= 8:
                n = 1 << depth
                pal = bytes(rng.randrange(256) for _ in range(3 * n))
                p = f"{out}/pal{depth}-{w}.png"
                raw_png(p, w, h, depth, 3, [pack_bits(r, depth) for r in px], chunk(b"PLTE", pal)); made.append((p, False))
                p = f"{out}/pal{depth}trns-{w}.png"
                trns = bytes(rng.randrange(256) for _ in range(n // 2 or 1))
                raw_png(p, w, h, depth, 3, [pack_bits(r, depth) for r in px], chunk(b"PLTE", pal) + chunk(b"tRNS", trns)); made.append((p, False))
        for depth in (8, 16):
            fmt = "B" if depth == 8 else ">H"
            mx = (1 << depth) - 1
            for colour, ch in ((2, 3), (6, 4), (4, 2)):
                px = [[rng.randrange(mx + 1) for _ in range(w * ch)] for _ in range(h)]
                if colour == 6:  # alpha with its edges: fully clear, fully opaque, in between
                    for r in px:
                        for i in range(3, len(r), 4):
                            r[i] = rng.choice([0, mx, rng.randrange(mx + 1)])
                rows = [b"".join(struct.pack(fmt, v) for v in r) for r in px]
                p = f"{out}/c{colour}-{depth}-{w}.png"; raw_png(p, w, h, depth, colour, rows); made.append((p, colour == 4))
                if colour == 2:
                    p = f"{out}/c2trns-{depth}-{w}.png"
                    raw_png(p, w, h, depth, 2, rows, chunk(b"tRNS", struct.pack(">HHH", *[v if depth == 16 else v for v in px[0][:3]]))); made.append((p, False))
    # Palette indices past the end of PLTE (invalid, but files carry them).
    p = f"{out}/pal-short.png"
    raw_png(p, 64, 64, 8, 3, [bytes(rng.randrange(256) for _ in range(64)) for _ in range(64)], chunk(b"PLTE", bytes(range(30)))); made.append((p, None))
    im = Image.new("RGB", (40, 30), (10, 20, 30)); im.save(f"{out}/anim.gif"); made.append((f"{out}/anim.gif", True))
    return made


cases = [(p, False) for p in files] + crafted()
same = diff = delegated = wrong_delegate = 0
for path, may_delegate in cases:
    for size in (128, 64):
        try:
            ref = prepare(path, size)
        except Exception as e:
            ref = e
        res = subprocess.run([accel, "__material-probe", "pixels", path, str(size)], capture_output=True, text=True)
        if res.returncode == DELEGATE:
            if may_delegate is False:
                wrong_delegate += 1
                print("DELEGATED", os.path.basename(path), size, getattr(ref, "mode", ref))
            delegated += 1
            continue
        if isinstance(ref, Exception) or ref is None:
            diff += 1
            print("PORT ANSWERED, ORIGINAL", os.path.basename(path), repr(ref)[:80])
            continue
        if ref.mode not in ("RGB", "RGBA"):
            diff += 1
            print("PORT ANSWERED, MODE", os.path.basename(path), ref.mode)
            continue
        c, s, h = stats(ref)
        want = [f"{ref.width} {ref.height} {len(ref.mode)} {fnv(ref.tobytes())}", f"{c!r} {s!r} {h!r}"]
        got = res.stdout.split("\n")[:2]
        got[1] = " ".join(got[1].split()[:3]) if len(got) > 1 else ""
        if got == want:
            same += 1
        else:
            diff += 1
            if diff < 10:
                print("DIFF", os.path.basename(path), size, ref.mode, want, got)
print("pixels same", same, "diff", diff, "delegated", delegated, "wrongly delegated", wrong_delegate)
