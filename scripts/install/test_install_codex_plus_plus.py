#!/usr/bin/env python3

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


INSTALL_SCRIPT = Path(__file__).with_name("install-codex-plus-plus.ps1")


@unittest.skipUnless(os.name == "nt", "Windows installer test")
class InstallCodexPlusPlusTest(unittest.TestCase):
    def test_install_rejects_a_release_store_inside_the_source_package(self) -> None:
        with tempfile.TemporaryDirectory(ignore_cleanup_errors=True) as temp_dir:
            package_dir = create_package(Path(temp_dir) / "package")
            result = invoke_installer(package_dir, package_dir / "bin", package_dir)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(
                "managed releases must be outside the source package",
                result.stderr,
            )

    def test_install_switches_generations_and_prunes_only_unlocked_releases(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory(ignore_cleanup_errors=True) as temp_dir:
            root = Path(temp_dir)
            package_dir = create_package(root / "package")
            shim_dir = root / "install" / "bin"
            codex_home = root / "codex-home"

            run_installer(package_dir, shim_dir, codex_home)
            first_release = only_release(codex_home)
            shim = (shim_dir / "codex.ps1").read_text(encoding="utf-8-sig")
            self.assertNotIn(str(package_dir), shim)
            self.assertIn(".codex-plus-plus-target", shim)
            self.assertEqual(
                Path((shim_dir / ".codex-plus-plus-target").read_text().strip()),
                first_release / "bin",
            )
            launched = subprocess.run(
                [str(shim_dir / "codex.cmd"), "/c", "exit", "0"],
                capture_output=True,
                check=False,
            )
            self.assertEqual(launched.returncode, 0, launched.stderr)

            with (first_release / "bin" / "codex.exe").open("rb"):
                run_installer(package_dir, shim_dir, codex_home)
                releases = release_dirs(codex_home)
                self.assertEqual(len(releases), 2)
                self.assertIn(first_release, releases)

            run_installer(package_dir, shim_dir, codex_home)
            releases = release_dirs(codex_home)
            self.assertEqual(len(releases), 1)
            self.assertEqual(
                Path((shim_dir / ".codex-plus-plus-target").read_text().strip()),
                releases[0] / "bin",
            )


def create_package(package_dir: Path) -> Path:
    (package_dir / "bin").mkdir(parents=True)
    (package_dir / "codex-package.json").write_text("{}\n", encoding="utf-8")
    shutil.copy2(os.environ["COMSPEC"], package_dir / "bin" / "codex.exe")
    return package_dir


def run_installer(package_dir: Path, shim_dir: Path, codex_home: Path) -> None:
    result = invoke_installer(package_dir, shim_dir, codex_home)
    if result.returncode != 0:
        raise AssertionError(f"installer failed:\n{result.stdout}\n{result.stderr}")


def invoke_installer(
    package_dir: Path,
    shim_dir: Path,
    codex_home: Path,
) -> subprocess.CompletedProcess[str]:
    env = os.environ.copy()
    env["CODEX_HOME"] = str(codex_home)
    return subprocess.run(
        [
            "powershell.exe",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            str(INSTALL_SCRIPT),
            "-TargetExe",
            str(package_dir / "bin" / "codex.exe"),
            "-ShimDir",
            str(shim_dir),
            "-Install",
        ],
        capture_output=True,
        check=False,
        env=env,
        text=True,
    )


def release_dirs(codex_home: Path) -> list[Path]:
    releases_dir = codex_home / "packages" / "codex-plus-plus" / "releases"
    return sorted(
        path for path in releases_dir.iterdir() if not path.name.startswith(".")
    )


def only_release(codex_home: Path) -> Path:
    releases = release_dirs(codex_home)
    if len(releases) != 1:
        raise AssertionError(f"expected one release, got {releases}")
    return releases[0]


if __name__ == "__main__":
    unittest.main()
