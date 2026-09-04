#!/usr/bin/env python3

from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch


REPO_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

import build_codex_plus_plus as build  # noqa: E402
from build_codex_plus_plus import CODEX_RS  # noqa: E402
from build_codex_plus_plus import default_package_args  # noqa: E402
from build_codex_plus_plus import next_fork_version  # noqa: E402
from build_codex_plus_plus import package_dir_arg  # noqa: E402
from build_codex_plus_plus import replace_package_version  # noqa: E402
from build_codex_plus_plus import versioned_package_manifest  # noqa: E402


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

    def test_fork_version_changes_only_the_entrypoint_package(self) -> None:
        manifest = """[package]
name = "codex-cli"
version.workspace = true
edition.workspace = true

[dependencies]
version = "not-a-package-version"
"""

        self.assertEqual(
            replace_package_version(manifest, "0.144.4-fork.3"),
            """[package]
name = "codex-cli"
version = "0.144.4-fork.3"
edition.workspace = true

[dependencies]
version = "not-a-package-version"
""",
        )

    def test_versioned_manifest_follows_package_variant(self) -> None:
        self.assertEqual(
            versioned_package_manifest([]), CODEX_RS / "cli" / "Cargo.toml"
        )
        self.assertEqual(
            versioned_package_manifest(["--variant", "codex-app-server"]),
            CODEX_RS / "app-server" / "Cargo.toml",
        )

    def test_build_restores_crlf_manifest_byte_for_byte_and_lockfile(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            manifest = root / "codex-rs" / "cli" / "Cargo.toml"
            lockfile = root / "codex-rs" / "Cargo.lock"
            manifest.parent.mkdir(parents=True)
            original_manifest = (
                b'[package]\r\nname = "codex-cli"\r\nversion.workspace = true\r\n'
            )
            manifest.write_bytes(original_manifest)
            lockfile.write_text("original lock\n", encoding="utf-8")

            def build_package(command: list[str], **_kwargs: object) -> int:
                self.assertIn(b"0.153.1-fork.2", manifest.read_bytes())
                self.assertEqual(command[-2:], ["--package-version", "0.153.1-fork.2"])
                lockfile.write_text("updated lock\n", encoding="utf-8")
                return 0

            with (
                patch.object(build, "REPO_ROOT", root),
                patch.object(build, "CARGO_LOCK", lockfile),
                patch.dict(
                    build.VERSIONED_PACKAGE_MANIFESTS,
                    {"codex": manifest},
                    clear=True,
                ),
                patch.object(build, "read_workspace_version", return_value="0.153.1"),
                patch.object(build.subprocess, "call", side_effect=build_package),
                patch.object(
                    sys,
                    "argv",
                    [
                        "build_codex_plus_plus.py",
                        "--fork-version",
                        "0.153.1-fork.2",
                        "--",
                        "--package-dir",
                        "dist/package",
                    ],
                ),
            ):
                self.assertEqual(build.main(), 0)

            self.assertEqual(manifest.read_bytes(), original_manifest)
            self.assertEqual(lockfile.read_text(), "original lock\n")


if __name__ == "__main__":
    unittest.main()
