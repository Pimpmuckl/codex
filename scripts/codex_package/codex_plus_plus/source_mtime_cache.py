"""Preserve source mtimes across restored Cargo target-directory caches."""

import hashlib
import json
import os
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path


STATE_VERSION = 1
HASH_CHUNK_SIZE = 1024 * 1024
EXCLUDED_DIRECTORY_NAMES = {".git", "target"}


@dataclass(frozen=True)
class RestoreResult:
    restored: int
    changed: int
    missing_state: bool = False
    invalid_state: bool = False


def source_files(root: Path) -> list[Path]:
    """Return regular source files below root, excluding build output trees."""
    files: list[Path] = []
    for directory, directory_names, file_names in os.walk(root):
        directory_names[:] = sorted(
            name for name in directory_names if name not in EXCLUDED_DIRECTORY_NAMES
        )
        directory_path = Path(directory)
        for name in sorted(file_names):
            path = directory_path / name
            if path.is_symlink() or not path.is_file():
                continue
            files.append(path)
    return files


def file_digest(path: Path) -> str:
    """Return the SHA-256 digest for one source file."""
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(HASH_CHUNK_SIZE), b""):
            digest.update(chunk)
    return digest.hexdigest()


def restore_source_mtimes(root: Path, state_path: Path) -> RestoreResult:
    """Restore mtimes only for files whose content still matches cached state."""
    try:
        state = json.loads(state_path.read_text(encoding="utf-8"))
        files_state = validate_state(state)
    except FileNotFoundError:
        print(f"No source mtime cache at {state_path}")
        return RestoreResult(restored=0, changed=0, missing_state=True)
    except (OSError, UnicodeError, json.JSONDecodeError, ValueError) as error:
        print(
            f"Ignoring invalid source mtime cache {state_path}: {error}",
            file=sys.stderr,
        )
        return RestoreResult(restored=0, changed=0, invalid_state=True)

    root = root.resolve()
    now_ns = time.time_ns()
    restored = 0
    changed = 0
    for path in source_files(root):
        relative = path.relative_to(root).as_posix()
        cached = files_state.get(relative)
        if cached is None or cached["sha256"] != file_digest(path):
            changed += 1
            continue

        cached_mtime_ns = cached["mtime_ns"]
        restored_mtime_ns = min(cached_mtime_ns, now_ns)
        stat = path.stat()
        os.utime(path, ns=(stat.st_atime_ns, restored_mtime_ns))
        restored += 1

    print(
        f"Source mtime cache restored {restored} unchanged files; "
        f"left {changed} changed or new files untouched"
    )
    return RestoreResult(restored=restored, changed=changed)


def save_source_mtimes(root: Path, state_path: Path) -> int:
    """Atomically record content digests and mtimes for the current source tree."""
    root = root.resolve()
    files: dict[str, dict[str, int | str]] = {}
    for path in source_files(root):
        relative = path.relative_to(root).as_posix()
        files[relative] = {
            "sha256": file_digest(path),
            "mtime_ns": path.stat().st_mtime_ns,
        }

    state_path.parent.mkdir(parents=True, exist_ok=True)
    payload = (
        json.dumps(
            {"version": STATE_VERSION, "files": files},
            sort_keys=True,
            separators=(",", ":"),
        )
        + "\n"
    )
    temporary_path: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w",
            encoding="utf-8",
            newline="\n",
            prefix=f".{state_path.name}.",
            suffix=".tmp",
            dir=state_path.parent,
            delete=False,
        ) as output:
            output.write(payload)
            temporary_path = Path(output.name)
        os.replace(temporary_path, state_path)
    finally:
        if temporary_path is not None:
            temporary_path.unlink(missing_ok=True)

    print(f"Source mtime cache recorded {len(files)} files at {state_path}")
    return len(files)


def validate_state(state: object) -> dict[str, dict[str, int | str]]:
    """Validate and normalize decoded source-mtime cache state."""
    if not isinstance(state, dict) or state.get("version") != STATE_VERSION:
        raise ValueError("unsupported state version")
    files = state.get("files")
    if not isinstance(files, dict):
        raise ValueError("state files must be an object")

    validated: dict[str, dict[str, int | str]] = {}
    for relative, entry in files.items():
        if not isinstance(relative, str) or not isinstance(entry, dict):
            raise ValueError("invalid state file entry")
        digest = entry.get("sha256")
        mtime_ns = entry.get("mtime_ns")
        if (
            not isinstance(digest, str)
            or len(digest) != 64
            or any(character not in "0123456789abcdef" for character in digest)
        ):
            raise ValueError(f"invalid digest for {relative!r}")
        if not isinstance(mtime_ns, int) or isinstance(mtime_ns, bool) or mtime_ns < 0:
            raise ValueError(f"invalid mtime for {relative!r}")
        validated[relative] = {"sha256": digest, "mtime_ns": mtime_ns}
    return validated
