#!/usr/bin/env python3

import json
import sys
import tempfile
import unittest
from pathlib import Path


sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from codex_package.codex_plus_plus.profile_benchmark import PLATFORMS  # noqa: E402
from codex_package.codex_plus_plus.profile_benchmark import load_samples  # noqa: E402
from codex_package.codex_plus_plus.profile_benchmark import render_report  # noqa: E402


def sample(platform: str, mode: str) -> dict[str, object]:
    return {
        "archive_bytes": 20 * 1024 * 1024,
        "cache_hit": mode == "warm",
        "executable_bytes": 10 * 1024 * 1024,
        "mode": mode,
        "platform": platform,
        "profile": "release",
        "repetition": 1,
        "target": PLATFORMS[platform],
        "wall_seconds": 100 if mode == "cold" else 40,
    }


class ProfileBenchmarkTest(unittest.TestCase):
    def test_report_compares_six_release_samples(self) -> None:
        samples = [
            sample(platform, mode)
            for platform in PLATFORMS
            for mode in ("cold", "warm")
        ]

        report = render_report(samples)

        self.assertIn("cold 100.0s → warm 40.0s (60.0% faster)", report)
        self.assertIn("3/3 warm builds restored exact caches", report)
        self.assertNotIn("release-fast", report)

    def test_loader_rejects_a_warm_cache_miss(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            for platform in PLATFORMS:
                for mode in ("cold", "warm"):
                    record = sample(platform, mode)
                    if platform == "linux" and mode == "warm":
                        record["cache_hit"] = False
                    path = root / f"{platform}-{mode}.json"
                    path.write_text(json.dumps(record), encoding="utf-8")

            with self.assertRaisesRegex(RuntimeError, "Invalid cache result"):
                load_samples(root)


if __name__ == "__main__":
    unittest.main()
