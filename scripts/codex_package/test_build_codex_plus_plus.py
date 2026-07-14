#!/usr/bin/env python3

from pathlib import Path
import sys
import unittest
from unittest.mock import patch


sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from build_codex_plus_plus import default_package_args  # noqa: E402
from build_codex_plus_plus import next_fork_version  # noqa: E402
from build_codex_plus_plus import package_dir_arg  # noqa: E402


class BuildCodexPlusPlusTest(unittest.TestCase):
    def test_default_version_advances_the_fork_release(self) -> None:
        with patch("build_codex_plus_plus.subprocess.run") as run:
            run.return_value.stdout = "\n".join(
                [
                    "codex-plus-plus-v0.144.4-fork.1",
                    "codex-plus-plus-v0.144.4-fork.2",
                    "codex-plus-plus-v0.144.3-fork.9",
                ]
            )

            self.assertEqual(next_fork_version("0.144.4"), "0.144.4-fork.3")

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
