#!/usr/bin/env python3
"""Prepare public trial jobs offline and record physical measurements explicitly."""
import argparse
import csv
import json
import math
import os
from pathlib import Path
import platform
import statistics
import re
import subprocess
from datetime import datetime, timezone

CASES = {"sparse": 3, "qr": 3, "repeated-batch": 30, "dense-qr": 10}


def prepare(args):
    if args.commit and not re.fullmatch(r"[0-9a-f]{40}", args.commit):
        raise ValueError("--commit must be the full tested source SHA")
    args.output.mkdir(parents=True, exist_ok=False)
    env = os.environ.copy()
    env.pop("THERMARK_ADDR", None)
    env["THERMARK_CONFIG"] = str(args.output.resolve() / "isolated-config.json")
    for case, count in CASES.items():
        csv_path = args.output / f"{case}.csv"
        with csv_path.open("w", newline="") as file:
            writer = csv.writer(file)
            writer.writerow(["text"] if case == "sparse" else ["text", "url"])
            for row in range(1, count + 1):
                text = f"TRIAL {row}"
                writer.writerow([text] if case == "sparse" else [text, "https://example.com/trial/" + ("dense/" * 15 if case == "dense-qr" else "") + str(row)])
        subprocess.run([args.binary, "batch", "--csv", str(csv_path), "--label", args.label, "--preview-dir", str(args.output / case)], check=True, env=env)
    version = subprocess.check_output([args.binary, "--version"], text=True).strip()
    metadata = {"schema_version": 1, "build": version, "tested_commit": args.commit, "os": platform.system(), "os_version": platform.release(), "arch": platform.machine(), "label": args.label, "cases": CASES, "evidence": "prepared offline; no physical results yet"}
    (args.output / "trial.json").write_text(json.dumps(metadata, indent=2) + "\n")
    print("Prepared public demo jobs. Use `thermark job print --job <case-directory>` after inspecting previews and loading matching media.")


def record(args):
    metadata = json.loads((args.output / "trial.json").read_text())
    expected = metadata["cases"][args.case]
    if not 0 <= args.scanned <= args.printed <= expected or not math.isfinite(args.seconds) or args.seconds < 0 or (args.case == "sparse" and args.scanned != 0):
        raise ValueError("counts must satisfy 0 <= scanned <= printed <= expected; seconds must be nonnegative")
    event = {"timestamp": datetime.now(timezone.utc).isoformat(), "case": args.case, "expected": expected, "printed": args.printed, "scanned": args.scanned, "seconds": args.seconds, "battery": args.battery, "condition": args.condition, "evidence": "operator-reported physical measurements"}
    with (args.output / "results.jsonl").open("a") as file:
        file.write(json.dumps(event) + "\n")
        file.flush()
        os.fsync(file.fileno())


def report(args):
    path = args.output / "results.jsonl"
    rows = [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []
    summary = {"trial": json.loads((args.output / "trial.json").read_text()), "physical_trials": len(rows), "cases": {}}
    for case in CASES:
        measurements = [r for r in rows if r["case"] == case]
        summary["cases"][case] = {"trials": len(measurements), "printed": sum(r["printed"] for r in measurements), "scanned": sum(r["scanned"] for r in measurements), "median_seconds": statistics.median(r["seconds"] for r in measurements) if measurements else None}
    print(json.dumps(summary, indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True, help="Private trial directory, e.g. local/trial-1")
    sub = parser.add_subparsers(dest="action", required=True)
    prep = sub.add_parser("prepare")
    prep.add_argument("--binary", default="thermark")
    prep.add_argument("--label", default="50x30")
    prep.add_argument("--commit", help="Full tested source SHA when known; never inferred from an unrelated checkout")
    rec = sub.add_parser("record")
    rec.add_argument("--case", choices=CASES, required=True)
    rec.add_argument("--printed", type=int, required=True)
    rec.add_argument("--scanned", type=int, required=True)
    rec.add_argument("--seconds", type=float, required=True)
    rec.add_argument("--battery", choices=["charged", "low", "unknown"], default="unknown")
    rec.add_argument("--condition", choices=["normal", "interrupted", "reconnected"], default="normal")
    sub.add_parser("report")
    args = parser.parse_args()
    {"prepare": prepare, "record": record, "report": report}[args.action](args)


if __name__ == "__main__":
    main()
