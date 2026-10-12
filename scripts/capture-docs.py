#!/usr/bin/env python3
"""Capture the local documentation page at fixed desktop and mobile sizes."""
import argparse
from functools import partial
import hashlib
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import shutil
import tempfile
from threading import Thread

ROOT = Path(__file__).resolve().parents[1]
VIEWPORTS = {"desktop": {"width": 1440, "height": 1000}, "mobile": {"width": 390, "height": 844}}


class QuietHandler(SimpleHTTPRequestHandler):
    def log_message(self, *_args):
        pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "local/screenshots", help="Fresh output directory")
    parser.add_argument("--browser", help="Chromium executable (default: system Chromium or Playwright's browser)")
    args = parser.parse_args()
    if args.output.exists():
        parser.error("Use a fresh output directory to preserve previous screenshots")
    try:
        from playwright.sync_api import sync_playwright
    except ImportError:
        parser.error("Install the optional screenshot dependency: python3 -m pip install playwright")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    server = ThreadingHTTPServer(("127.0.0.1", 0), partial(QuietHandler, directory=str(ROOT / "docs")))
    Thread(target=server.serve_forever, daemon=True).start()
    try:
        with tempfile.TemporaryDirectory(prefix=".screenshots-", dir=args.output.parent) as temporary:
            staging = Path(temporary)
            with sync_playwright() as playwright:
                executable = args.browser or shutil.which("chromium") or shutil.which("chromium-browser")
                with playwright.chromium.launch(executable_path=executable, args=["--no-sandbox"]) as browser:
                    metadata = {"browser": browser.version, "page": "docs/index.html",
                                "page_sha256": hashlib.sha256((ROOT / "docs/index.html").read_bytes()).hexdigest(),
                                "device_scale_factor": 1, "viewports": VIEWPORTS, "images": {}}
                    for name, viewport in VIEWPORTS.items():
                        page = browser.new_page(viewport=viewport, device_scale_factor=1, reduced_motion="reduce")
                        page.goto(f"http://127.0.0.1:{server.server_port}/", wait_until="networkidle")
                        page.evaluate("document.fonts.ready")
                        if not page.evaluate("Array.from(document.images).every(i => i.complete && i.naturalWidth > 0)"):
                            raise RuntimeError("Documentation has missing images")
                        if page.evaluate("document.documentElement.scrollWidth > window.innerWidth"):
                            raise RuntimeError(f"Documentation overflows the {name} viewport")
                        page.screenshot(path=str(staging / f"{name}.png"), full_page=True, animations="disabled")
                        metadata["images"][name] = hashlib.sha256((staging / f"{name}.png").read_bytes()).hexdigest()
                        page.close()
            (staging / "capture.json").write_text(json.dumps(metadata, indent=2) + "\n")
            # Publish only a complete capture set, retaining earlier directories.
            staging.rename(args.output)
    finally:
        server.shutdown()
        server.server_close()
    print(f"Captured desktop/mobile screenshots and browser metadata in {args.output}")


if __name__ == "__main__":
    main()
