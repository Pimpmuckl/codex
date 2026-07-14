#!/usr/bin/env python3

from pathlib import Path
import sys
import unittest


sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from build_codex_plus_plus import default_package_args  # noqa: E402
from build_codex_plus_plus import package_dir_arg  # noqa: E402


class BuildCodexPlusPlusTest(unittest.TestCase):
    def test_forwarded_options_use_the_final_value(self) -> None:
        self.assertEqual(
            package_dir_arg(["--package-dir", "first", "--package-d=second"]),
            Path("second"),
        )
        self.assertEqual(
            default_package_args(["--package-d=custom"]),
            ["--cargo-profile", "release-fast", "--package-d=custom"],
        )


if __name__ == "__main__":
    unittest.main()
