#!/usr/bin/env python3
"""Refresh documentation PNGs from the current CLI, or check them without edits."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
RECIPES = ("guest-wifi", "inventory", "equipment", "return", "badge", "storage", "packing", "batch")
IMAGES = ("hello-label.png", *(f"recipes/{name}.png" for name in RECIPES))


def run(*args, env=None):
    return subprocess.run(args, cwd=ROOT, env=env, check=True, capture_output=True, text=True).stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="Fail on missing/stale images; write nothing")
    parser.add_argument("--output", type=Path, default=ROOT / "docs/demo", help="Image directory (default: docs/demo)")
    args = parser.parse_args()
    # Always build this checkout; an installed release may render differently.
    subprocess.run(["cargo", "build", "--locked", "--no-default-features", "--bin", "thermark"], cwd=ROOT, check=True)
    metadata = json.loads(run("cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"))
    binary = Path(metadata["target_directory"]) / "debug" / "thermark"
    with tempfile.TemporaryDirectory(prefix="thermark-docs-") as temporary:
        staging = Path(temporary)
        env = os.environ.copy()
        env.pop("THERMARK_ADDR", None)
        env.pop("THERMARK_FONT", None)
        env.pop("THERMARK_WIFI_PASSWORD", None)
        env["THERMARK_CONFIG"] = str(staging / "config.json")
        env["THERMARK_BIN"] = str(binary)
        run("sh", str(ROOT / "examples/render-recipes.sh"), str(staging / "recipes"), env=env)
        run(str(binary), "qr", "--url", "https://example.com", "--text", "Hello, label!",
            "--model", "b1", "--label", "50x30", "--save", str(staging / "hello-label.png"), "--no-print", env=env)
        if (staging / "config.json").exists():
            raise RuntimeError("Offline rendering unexpectedly wrote printer configuration")
        hashes = {}
        for name in IMAGES:
            data = (staging / name).read_bytes()
            if data[:16] != b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR" or struct.unpack(">II", data[16:24]) != (384, 240):
                raise RuntimeError(f"Expected a 384x240 PNG: {name}")
            hashes[name] = hashlib.sha256(data).hexdigest()
        manifest = {"schema_version": 1, "build": run(str(binary), "--version", env=env).strip(),
                    "model": "b1", "label_mm": "50x30", "font": "embedded DejaVu Sans",
                    "font_sha256": hashlib.sha256((ROOT / "assets/fonts/DejaVuSans.ttf").read_bytes()).hexdigest(),
                    "images": hashes}
        (staging / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
        files = (*IMAGES, "manifest.json")
        changed = [name for name in files if not (args.output / name).is_file()
                   or (args.output / name).read_bytes() != (staging / name).read_bytes()]
        if args.check:
            if changed:
                parser.exit(1, "Stale documentation images: " + ", ".join(changed)
                            + "\nRun python3 scripts/render-docs.py and review the PNGs.\n")
            print(f"Verified {len(IMAGES)} documentation images and their manifest")
        else:
            # Rendering and validation finish before any existing image changes.
            for name in changed:
                destination = args.output / name
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(staging / name, destination)
            print(f"Refreshed {len(changed)} files in {args.output}; review git diff and the PNGs")


if __name__ == "__main__":
    main()
