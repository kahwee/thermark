import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("package", Path(__file__).with_name("check-package.py"))
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)


class InventoryTests(unittest.TestCase):
    def test_required_inventory(self):
        package.validate(package.REQUIRED)
        for path in package.REQUIRED:
            with self.subTest(missing=path), self.assertRaises(ValueError):
                package.validate(package.REQUIRED - {path})

    def test_private_and_infrastructure_files_are_rejected(self):
        for path in ("local/wifi.png", "infra/runner.yml", ".github/workflows/ci.yml", ".agents/skills/release/SKILL.md", "target/debug/app"):
            with self.subTest(unexpected=path), self.assertRaises(ValueError):
                package.validate(package.REQUIRED | {path})


if __name__ == "__main__":
    unittest.main()
