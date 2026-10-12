import importlib.util
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest

spec = importlib.util.spec_from_file_location("trial", Path(__file__).with_name("hardware-trial.py"))
trial = importlib.util.module_from_spec(spec)
spec.loader.exec_module(trial)


class TrialTests(unittest.TestCase):
    def test_rejects_impossible_measurements_without_writing_receipts(self):
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp)
            (output / "trial.json").write_text(json.dumps({"cases": trial.CASES}))
            args = SimpleNamespace(output=output, case="qr", printed=1, scanned=2, seconds=1, battery="charged", condition="normal")
            with self.assertRaises(ValueError):
                trial.record(args)
            self.assertFalse((output / "results.jsonl").exists())
            args.scanned = 1
            trial.record(args)
            event = json.loads((output / "results.jsonl").read_text())
            self.assertEqual(event["scanned"], 1)
            self.assertEqual(event["evidence"], "operator-reported physical measurements")


if __name__ == "__main__":
    unittest.main()
