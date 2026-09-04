#!/usr/bin/env python3

import json
import os
from pathlib import Path
import sys
import tempfile
import time
import unittest


sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from codex_package.codex_plus_plus.source_mtime_cache import (  # noqa: E402
    RestoreResult,
)
from codex_package.codex_plus_plus.source_mtime_cache import (  # noqa: E402
    restore_source_mtimes,
)
from codex_package.codex_plus_plus.source_mtime_cache import (  # noqa: E402
    save_source_mtimes,
)


class SourceMtimeCacheTest(unittest.TestCase):
    def test_restores_only_content_identical_sources(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / "codex-rs"
            state = Path(temp) / "source-mtimes.json"
            unchanged = root / "core" / "src" / "lib.rs"
            changed = root / "cli" / "src" / "main.rs"
            ignored = root / "target" / "release" / "codex"
            for path in (unchanged, changed, ignored):
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(path.name, encoding="utf-8")

            old_unchanged_mtime = 1_650_000_000_000_000_000
            old_changed_mtime = old_unchanged_mtime + 1_000_000_000
            os.utime(unchanged, ns=(old_unchanged_mtime, old_unchanged_mtime))
            os.utime(changed, ns=(old_changed_mtime, old_changed_mtime))
            self.assertEqual(save_source_mtimes(root, state), 2)

            checkout_mtime = time.time_ns() - 2_000_000_000
            os.utime(unchanged, ns=(checkout_mtime, checkout_mtime))
            changed.write_text("changed content", encoding="utf-8")
            os.utime(changed, ns=(checkout_mtime, checkout_mtime))

            result = restore_source_mtimes(root, state)

            self.assertEqual(result, RestoreResult(restored=1, changed=1))
            self.assertEqual(unchanged.stat().st_mtime_ns, old_unchanged_mtime)
            self.assertEqual(changed.stat().st_mtime_ns, checkout_mtime)
            saved = json.loads(state.read_text(encoding="utf-8"))
            self.assertNotIn("target/release/codex", saved["files"])

    def test_missing_and_invalid_state_are_cache_misses(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / "codex-rs"
            root.mkdir()
            state = Path(temp) / "source-mtimes.json"

            missing = restore_source_mtimes(root, state)
            self.assertEqual(
                missing,
                RestoreResult(restored=0, changed=0, missing_state=True),
            )

            state.write_text('{"version":99,"files":{}}\n', encoding="utf-8")
            invalid = restore_source_mtimes(root, state)
            self.assertEqual(
                invalid,
                RestoreResult(restored=0, changed=0, invalid_state=True),
            )


if __name__ == "__main__":
    unittest.main()
