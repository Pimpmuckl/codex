#!/usr/bin/env python3

import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch


REPO_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO_ROOT / "codex-cli" / "scripts"))

import build_npm_package as build  # noqa: E402


class NpmPackageBuilderTest(unittest.TestCase):
    def test_run_npm_pack_accepts_npm_11_and_12_json_shapes(self) -> None:
        tarball_name = "jjliebig-codex-plus-plus-0.144.4-fork.2.tgz"
        pack_entry = {"filename": tarball_name}
        outputs = ([pack_entry], {"@jjliebig/codex-plus-plus": pack_entry})

        with tempfile.TemporaryDirectory() as temp_dir:
            temp = Path(temp_dir)
            staging_dir = temp / "stage"
            staging_dir.mkdir()

            for index, pack_output in enumerate(outputs):
                with self.subTest(pack_output=pack_output):
                    output_path = temp / f"output-{index}.tgz"

                    def npm_pack(command, **_kwargs):
                        pack_dir = Path(
                            command[command.index("--pack-destination") + 1]
                        )
                        (pack_dir / tarball_name).write_bytes(b"tarball")
                        return json.dumps(pack_output)

                    with patch.object(
                        build.subprocess, "check_output", side_effect=npm_pack
                    ):
                        self.assertEqual(
                            build.run_npm_pack(staging_dir, output_path),
                            output_path.resolve(),
                        )
                    self.assertEqual(output_path.read_bytes(), b"tarball")


if __name__ == "__main__":
    unittest.main()
