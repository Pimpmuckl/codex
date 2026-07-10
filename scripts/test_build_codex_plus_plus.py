#!/usr/bin/env python3

import unittest
from unittest.mock import patch

from build_codex_plus_plus import builds_windows_package
from build_codex_plus_plus import with_crlf_line_endings


class BuildCodexPlusPlusTest(unittest.TestCase):
    def test_windows_target_uses_crlf_migrations(self) -> None:
        with patch(
            "build_codex_plus_plus.default_target",
            return_value="x86_64-pc-windows-msvc",
        ):
            self.assertTrue(builds_windows_package([]))

        self.assertTrue(builds_windows_package(["--target", "x86_64-pc-windows-msvc"]))
        self.assertFalse(builds_windows_package(["--target=aarch64-apple-darwin"]))

        self.assertEqual(
            with_crlf_line_endings(b"one\ntwo\r\nthree\rfour"),
            b"one\r\ntwo\r\nthree\r\nfour",
        )


if __name__ == "__main__":
    unittest.main()
