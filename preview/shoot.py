"""Screenshots of the real UI with a mocked backend.

    npx vite --config preview/vite.preview.config.ts      # other terminal
    python preview/shoot.py
"""
import os
from pathlib import Path

from playwright.sync_api import sync_playwright

OUT = Path(__file__).resolve().parent / "out"


def main():
    OUT.mkdir(exist_ok=True)
    with sync_playwright() as p:
        b = p.chromium.launch(channel="chrome", headless=True)
        for scheme in ("light", "dark"):
            page = b.new_page(viewport={"width": 1040, "height": 720}, color_scheme=scheme, device_scale_factor=1.5)
            page.goto("http://localhost:1460/", wait_until="domcontentloaded", timeout=120000)
            page.wait_for_selector(".culprit", timeout=120000)
            page.wait_for_timeout(700)
            page.click(".culprit >> nth=0")
            page.wait_for_timeout(300)
            page.screenshot(path=str(OUT / f"main-{scheme}.png"))
            page.click(".culprit:has-text('Google Chrome') .crow")
            page.click("text=End task…")
            page.wait_for_selector(".modal")
            page.screenshot(path=str(OUT / f"endtask-{scheme}.png"))
            page.close()
    print("saved", flush=True)
    os._exit(0)


if __name__ == "__main__":
    main()
