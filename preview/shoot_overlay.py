"""Headless screenshots + a short clip of the mini overlay on fake wallpapers.

    npx vite --config preview/vite.preview.config.ts      # other terminal
    python preview/shoot_overlay.py
"""
import os
import shutil
import subprocess
import time
from pathlib import Path

from playwright.sync_api import sync_playwright

OUT = Path(__file__).resolve().parent / "out"
BASE = "http://localhost:1460/preview/overlay-stage.html"


def main():
    OUT.mkdir(exist_ok=True)
    with sync_playwright() as p:
        b = p.chromium.launch(channel="chrome", headless=True)
        for theme in ("dark", "light"):
            page = b.new_page(viewport={"width": 720, "height": 300}, device_scale_factor=2)
            page.goto(f"{BASE}?theme={theme}&speed=6", wait_until="domcontentloaded", timeout=120000)
            fr = page.frame_locator("#ov")
            fr.locator(".row").first.wait_for(timeout=120000)
            page.wait_for_timeout(5600)  # ~17 samples at 6× speed: calm part, sparklines filling
            page.screenshot(path=str(OUT / f"overlay-{theme}.png"))
            fr.locator(".row.t-strained").first.wait_for(timeout=30000)  # wait for the storm's peak
            page.wait_for_timeout(700)
            page.screenshot(path=str(OUT / f"overlay-busy-{theme}.png"))
            page.hover("#ov")
            page.wait_for_timeout(300)
            page.screenshot(path=str(OUT / f"overlay-hover-{theme}.png"))
            page.close()

        # clip: one calm → storm → calm cycle at 2.5× speed
        for f in OUT.glob("*.webm"):
            f.unlink()
        ctx = b.new_context(viewport={"width": 720, "height": 300}, record_video_dir=str(OUT), record_video_size={"width": 720, "height": 300})
        page = ctx.new_page()
        t0 = time.time()
        page.goto(f"{BASE}?theme=dark&speed=2.5", wait_until="domcontentloaded", timeout=120000)
        page.frame_locator("#ov").locator(".row").first.wait_for(timeout=120000)
        skip = time.time() - t0
        page.wait_for_timeout(22000)
        video = page.video.path()
        ctx.close()
    raw = OUT / "overlay-raw.webm"
    shutil.move(video, raw)
    mp4, gif = OUT / "overlay.mp4", OUT / "overlay.gif"
    ff = ["ffmpeg", "-y", "-loglevel", "error"]
    subprocess.run(ff + ["-i", str(raw), "-ss", f"{skip:.2f}", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "20", "-preset", "fast", "-movflags", "+faststart", str(mp4)], check=True)
    subprocess.run(ff + ["-i", str(mp4), "-vf", "fps=12,scale=600:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=128[p];[b][p]paletteuse=dither=bayer:bayer_scale=4", str(gif)], check=True)
    for f in (mp4, gif):
        print(f"{f.name} {f.stat().st_size / 1e6:.1f} MB", flush=True)
    os._exit(0)


if __name__ == "__main__":
    main()
