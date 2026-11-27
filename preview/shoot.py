"""Screenshots of the real UI with a mocked backend (fictional data).

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
            for sc in ("pc", "wifi", "isp", "calm"):
                page = b.new_page(viewport={"width": 1100, "height": 800}, color_scheme=scheme, device_scale_factor=1.5)
                page.goto(f"http://localhost:1460/?scenario={sc}&speed=3", wait_until="domcontentloaded", timeout=120000)
                page.wait_for_selector(".verdict h1", timeout=120000)
                page.wait_for_timeout(5000)  # let sparklines fill
                if sc == "pc":
                    page.click(".culprit >> nth=0")
                    page.wait_for_timeout(300)
                page.screenshot(path=str(OUT / f"{sc}-{scheme}.png"))
                if sc == "pc" and scheme == "light":
                    page.click(".culprit:has-text('Google Chrome') .crow")
                    page.click("text=End task…")
                    page.wait_for_selector(".modal")
                    page.screenshot(path=str(OUT / f"endtask-{scheme}.png"))
                page.close()
    print("saved", flush=True)
    os._exit(0)


if __name__ == "__main__":
    main()
