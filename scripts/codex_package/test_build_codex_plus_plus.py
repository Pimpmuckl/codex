#!/usr/bin/env python3

from pathlib import Path
import sys
import unittest


sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from build_codex_plus_plus import option_value  # noqa: E402


class BuildCodexPlusPlusTest(unittest.TestCase):
    def test_forwarded_options_use_the_final_value(self) -> None:
        self.assertEqual(
            option_value(
                ["--package-dir", "first", "--package-dir=second"],
                "--package-dir",
            ),
            "second",
        )


if __name__ == "__main__":
    unittest.main()
