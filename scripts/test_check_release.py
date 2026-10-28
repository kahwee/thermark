import importlib.util
from pathlib import Path
import unittest
import subprocess
import sys

spec = importlib.util.spec_from_file_location("check", Path(__file__).with_name("check-release.py"))
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)


class MetadataTests(unittest.TestCase):
    def test_cli_validates_actual_checked_out_metadata_and_rejects_bad_tag(self):
        directory = Path(__file__).resolve().parent.parent
        manifest = check.tomllib.loads((directory / "Cargo.toml").read_text())
        script = directory / "scripts/check-release.py"
        version = manifest["package"]["version"]
        self.assertEqual(subprocess.run([sys.executable, str(script), "v" + version], cwd=directory, capture_output=True).returncode, 0)
        self.assertEqual(subprocess.run([sys.executable, str(script), "v0.0.0-invalid"], cwd=directory, capture_output=True).returncode, 1)

    def test_manifest_tag_and_lock_must_match(self):
        manifest = {"package": {"name": "example", "version": "1.2.3"}}
        lock = {"package": [{"name": "example", "version": "1.2.3"}]}
        self.assertEqual(check.validate(manifest, lock, "v1.2.3"), ("example", "1.2.3"))
        with self.assertRaises(ValueError):
            check.validate(manifest, lock, "v1.2.4")
        with self.assertRaises(ValueError):
            check.validate(manifest, {"package": [{"name": "example", "version": "1.2.2"}]}, "v1.2.3")
        with self.assertRaises(ValueError):
            check.validate(manifest, {"package": []}, "v1.2.3")

    def test_disabled_publication_and_unstable_versions_are_rejected(self):
        for version, publish in (("1.2.3", False), ("1.2.3-rc.1", True), ("1.2.3+build", True)):
            with self.subTest(version=version, publish=publish), self.assertRaises(ValueError):
                check.validate({"package": {"name": "example", "version": version, "publish": publish}},
                               {"package": [{"name": "example", "version": version}]}, "v" + version)

    def test_registry_dependency_cannot_substitute_for_root_lock_entry(self):
        manifest = {"package": {"name": "example", "version": "1.2.3"}}
        root = {"name": "example", "version": "1.2.3"}
        registry = dict(root, source="registry+https://github.com/rust-lang/crates.io-index")
        self.assertEqual(check.validate(manifest, {"package": [root, registry]}, "v1.2.3"), ("example", "1.2.3"))
        for entries in ([registry], [root, root]):
            with self.subTest(entries=entries), self.assertRaises(ValueError):
                check.validate(manifest, {"package": entries}, "v1.2.3")


if __name__ == "__main__":
    unittest.main()
