#!/usr/bin/env python3
"""Run and summarize the temporary Codex++ release cache benchmark."""

import argparse
import json
import subprocess
import sys
import time
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[3]
CODEX_RS_ROOT = REPO_ROOT / "codex-rs"
PROFILE = "release"
PLATFORMS = {
    "linux": "x86_64-unknown-linux-musl",
    "macos": "aarch64-apple-darwin",
    "windows": "x86_64-pc-windows-msvc",
}
MODES = ("cold", "warm")


def native_library_path(target: str) -> Path:
    name = "rusty_v8.lib" if target.endswith("-pc-windows-msvc") else "librusty_v8.a"
    return CODEX_RS_ROOT / "target" / target / PROFILE / "gn_out" / "obj" / name


def run_sample(args: argparse.Namespace) -> None:
    cache_hit = args.cache_hit == "true"
    if cache_hit != (args.mode == "warm"):
        raise RuntimeError(
            f"{args.mode} sample has invalid cache hit: {args.cache_hit}"
        )
    if PLATFORMS.get(args.platform) != args.target:
        raise RuntimeError(f"Target {args.target} does not match {args.platform}")

    package_args = [
        "--target",
        args.target,
        "--cargo-profile",
        PROFILE,
        "--package-dir",
        str(args.package_dir),
        "--archive-output",
        str(args.archive),
        "--force",
    ]
    started_at_ns = args.started_at_ns or time.time_ns()
    if args.platform == "linux":
        subprocess.run(
            [
                "cargo",
                "build",
                "--target",
                args.target,
                "--profile",
                PROFILE,
                "--bin",
                "bwrap",
            ],
            cwd=CODEX_RS_ROOT,
            check=True,
        )
        bwrap = CODEX_RS_ROOT / "target" / args.target / PROFILE / "bwrap"
        subprocess.run(
            ["strip", "--strip-debug", "--strip-unneeded", str(bwrap)],
            check=True,
        )
        package_args.extend(["--bwrap-bin", str(bwrap)])

    subprocess.run(
        [
            sys.executable,
            str(REPO_ROOT / "scripts" / "build_codex_plus_plus.py"),
            "--fork-version",
            args.version,
            "--",
            *package_args,
        ],
        cwd=REPO_ROOT,
        check=True,
    )

    executable = args.package_dir / args.executable
    version = subprocess.run(
        [str(executable), "--version"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    if version != f"codex-cli {args.version}":
        raise RuntimeError(f"Unexpected packaged CLI version: {version}")
    if args.mode == "warm" and not native_library_path(args.target).is_file():
        raise RuntimeError("Warm build did not recreate the rusty_v8 native library")

    record = {
        "archive_bytes": args.archive.stat().st_size,
        "cache_hit": cache_hit,
        "executable_bytes": executable.stat().st_size,
        "mode": args.mode,
        "platform": args.platform,
        "profile": PROFILE,
        "repetition": 1,
        "target": args.target,
        "wall_seconds": round((time.time_ns() - started_at_ns) / 1_000_000_000, 3),
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(record, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )


def load_samples(root: Path) -> list[dict[str, object]]:
    samples = [
        json.loads(path.read_text(encoding="utf-8"))
        for path in sorted(root.rglob("*.json"))
    ]
    expected = {(target, mode) for target in PLATFORMS.values() for mode in MODES}
    actual = {(str(sample["target"]), str(sample["mode"])) for sample in samples}
    if actual != expected or len(samples) != len(expected):
        raise RuntimeError(
            f"Expected samples {sorted(expected)}, found {sorted(actual)}"
        )

    for sample in samples:
        platform = str(sample["platform"])
        target = str(sample["target"])
        mode = str(sample["mode"])
        if PLATFORMS.get(platform) != target:
            raise RuntimeError(f"Target {target} does not match {platform}")
        if sample["profile"] != PROFILE or sample["repetition"] != 1:
            raise RuntimeError(f"Invalid release sample metadata: {sample}")
        if bool(sample["cache_hit"]) != (mode == "warm"):
            raise RuntimeError(f"Invalid cache result for {target}/{mode}")
    return samples


def render_report(samples: list[dict[str, object]]) -> str:
    by_key = {
        (str(sample["target"]), str(sample["mode"])): sample for sample in samples
    }
    lines = [
        "# Codex++ release cache benchmark",
        "",
        "| Platform / target | Mode | Cache hit | Wall | Executable | Archive |",
        "| --- | --- | ---: | ---: | ---: | ---: |",
    ]
    for platform, target in PLATFORMS.items():
        for mode in MODES:
            sample = by_key[(target, mode)]
            lines.append(
                f"| {platform} / `{target}` | {mode} | "
                f"{str(sample['cache_hit']).lower()} | "
                f"{float(sample['wall_seconds']):.1f}s | "
                f"{int(sample['executable_bytes']) / 1024 / 1024:.1f} MiB | "
                f"{int(sample['archive_bytes']) / 1024 / 1024:.1f} MiB |"
            )

    cold = max(
        float(by_key[(target, "cold")]["wall_seconds"]) for target in PLATFORMS.values()
    )
    warm = max(
        float(by_key[(target, "warm")]["wall_seconds"]) for target in PLATFORMS.values()
    )
    change = (cold - warm) / cold * 100
    comparison = f"{abs(change):.1f}% {'faster' if change >= 0 else 'slower'}"
    lines.extend(
        [
            "",
            f"**Critical path:** cold {cold:.1f}s → warm {warm:.1f}s ({comparison}).",
            "",
            "Warm wall time includes exact cache restore and the scoped v8 clean.",
            "",
            "**Cache mechanism:** exact `actions/cache` save/restore of Cargo registry/git "
            "data and the `release` target directories.",
            "",
            "**Cache proof:** 3/3 warm builds restored exact caches, recreated rusty_v8, "
            "and passed packaged CLI smoke.",
        ]
    )
    return "\n".join(lines) + "\n"


def aggregate(args: argparse.Namespace) -> None:
    args.output.write_text(render_report(load_samples(args.results)), encoding="utf-8")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(required=True)

    sample = subparsers.add_parser("sample")
    sample.add_argument("--platform", choices=sorted(PLATFORMS), required=True)
    sample.add_argument("--target", required=True)
    sample.add_argument("--mode", choices=MODES, required=True)
    sample.add_argument("--cache-hit", choices=("false", "true"), required=True)
    sample.add_argument("--version", required=True)
    sample.add_argument("--package-dir", type=Path, required=True)
    sample.add_argument("--archive", type=Path, required=True)
    sample.add_argument("--executable", type=Path, required=True)
    sample.add_argument("--output", type=Path, required=True)
    sample.add_argument("--started-at-ns", type=int)
    sample.set_defaults(func=run_sample)

    report = subparsers.add_parser("aggregate")
    report.add_argument("--results", type=Path, required=True)
    report.add_argument("--output", type=Path, required=True)
    report.set_defaults(func=aggregate)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
