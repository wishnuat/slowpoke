"""Record the Slowpoke demo video headlessly from the real UI (mocked backend, fictional data).

    npx vite --config preview/vite.preview.config.ts      # other terminal
    python demo-video/record.py
"""
import os
import shutil
import subprocess
import time
from pathlib import Path

from playwright.sync_api import sync_playwright

HERE = Path(__file__).resolve().parent
OUT = HERE / "out"

CAPTION = """([step, text, top]) => {
  let el = document.getElementById('cap');
  if (!el) {
    el = document.createElement('div'); el.id = 'cap';
    el.style.cssText = 'position:fixed;left:50%;bottom:70px;transform:translateX(-50%);z-index:99;max-width:820px;text-align:center;'
      + 'background:rgba(15,17,21,.92);color:#fff;padding:12px 22px;border-radius:12px;font:600 18px Segoe UI,system-ui;'
      + 'box-shadow:0 10px 30px rgba(0,0,0,.35);transition:opacity .25s;pointer-events:none';
    document.body.appendChild(el);
  }
  el.style.top = top ? '150px' : 'auto'; el.style.bottom = top ? 'auto' : '70px';
  el.style.opacity = text ? 1 : 0;
  if (text) el.innerHTML = (step ? '<span style="color:#7dd3fc;font-size:12px;letter-spacing:.08em;display:block;margin-bottom:4px">' + step + '</span>' : '') + text;
}"""

CURSOR = """() => {
  const c = document.createElement('div'); c.id = 'cur';
  c.innerHTML = '<svg viewBox="0 0 24 24" width="22" height="22"><path d="M3 2l7.5 19 2.6-7.9L21 10.5z" fill="#fff" stroke="#000" stroke-width="1.3" stroke-linejoin="round"/></svg>';
  c.style.cssText = 'position:fixed;left:0;top:0;z-index:100;pointer-events:none;transition:transform .4s cubic-bezier(.3,.7,.3,1);filter:drop-shadow(0 1px 2px rgba(0,0,0,.5));transform:translate(520px,360px)';
  document.body.appendChild(c);
  window.cur = (x, y) => { c.style.transform = `translate(${x}px, ${y}px)`; };
}"""


def main():
    OUT.mkdir(exist_ok=True)
    for f in OUT.glob("*.webm"):
        f.unlink()
    with sync_playwright() as p:
        b = p.chromium.launch(channel="chrome", headless=True)
        ctx = b.new_context(viewport={"width": 1100, "height": 800}, color_scheme="dark", record_video_dir=str(OUT), record_video_size={"width": 1100, "height": 800})
        page = ctx.new_page()
        t0 = time.time()
        page.goto("http://localhost:1460/?scenario=calm", wait_until="domcontentloaded", timeout=120000)
        page.wait_for_selector(".verdict h1", timeout=120000)
        page.evaluate(CURSOR)
        page.wait_for_timeout(600)
        skip = max(0.0, time.time() - t0 - 0.3)
        cap = lambda s, t, top=False: page.evaluate(CAPTION, [s, t, top])

        def move(sel, wait=500):
            bb = page.locator(sel).first.bounding_box()
            page.evaluate("([x, y]) => cur(x, y)", [bb["x"] + bb["width"] / 2, bb["y"] + bb["height"] / 2])
            page.wait_for_timeout(wait)

        def scenario(name):
            page.evaluate("(s) => window.__setScenario(s)", name)

        cap("Slowpoke", "Why is my PC, or my internet, slow? One sentence, and the culprit")
        page.wait_for_timeout(3400)
        cap("Step 1", "When the PC is the problem, it names the apps behind it…")
        scenario("pc")
        page.wait_for_timeout(1200)
        move(".culprit", 2800)
        cap("", "…and tells you the internet is fine, so you don't restart the router for nothing")
        move(".also", 3200)
        cap("Step 2", "Walk away from the router…", True)
        scenario("wifi")
        page.wait_for_timeout(1500)
        move(".node.culprit-hop", 2600)
        cap("", "…it jumps to the network, points at the Wi-Fi, and says your PC is calm", True)
        move(".also", 3400)
        cap("Step 3", "When the provider has trouble, it says so instead", True)
        scenario("isp")
        page.wait_for_timeout(1500)
        move(".node.culprit-hop", 3200)
        cap("Step 4", "Every slow moment is remembered, with its cause, for 7 days", True)
        scenario("calm")
        page.click(".tabs button >> nth=0")
        page.wait_for_timeout(400)
        move(".moments li", 3400)
        cap("Step 5", "A real speed test when you want one", True)
        page.click(".tabs button >> nth=1")
        page.wait_for_timeout(400)
        move("button.primary", 700)
        page.click("button.primary")
        page.wait_for_selector(".speedres", timeout=10000)
        page.wait_for_timeout(2600)
        cap("Slowpoke", "Lives in the tray · mini overlay · 0.1 % CPU")
        move(".overlaybtn", 3200)
        cap("", "")
        page.wait_for_timeout(400)
        video = page.video.path()
        ctx.close()  # finalizes the video

    webm = OUT / "raw.webm"
    shutil.move(video, webm)
    mp4, gif = OUT / "slowpoke-demo.mp4", OUT / "slowpoke-demo.gif"
    ff = ["ffmpeg", "-y", "-loglevel", "error"]
    subprocess.run(ff + ["-i", str(webm), "-ss", f"{skip:.2f}", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "21", "-preset", "fast", "-movflags", "+faststart", str(mp4)], check=True)
    subprocess.run(ff + ["-i", str(mp4), "-vf", "fps=10,scale=820:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=128[p];[b][p]paletteuse=dither=bayer:bayer_scale=4", str(gif)], check=True)
    for f in (mp4, gif):
        print(f"{f}  {f.stat().st_size / 1e6:.1f} MB", flush=True)
    os._exit(0)


if __name__ == "__main__":
    main()
