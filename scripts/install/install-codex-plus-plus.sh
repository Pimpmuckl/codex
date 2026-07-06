#!/bin/sh
set -eu

target_exe=
shim_dir="${HOME:-.}/.local/bin"
install=0
dry_run=0

usage() {
  cat <<'EOF'
Usage: install-codex-plus-plus.sh --target-exe PATH [--shim-dir DIR] [--install] [--dry-run]
EOF
}

abs_path() {
  case "$1" in
    /*) printf '%s\n' "$1" ;;
    *) printf '%s/%s\n' "$(pwd -P)" "$1" ;;
  esac
}

shell_quote() {
  printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"
}

npm_codex_version() {
  if ! command -v npm >/dev/null 2>&1; then
    printf 'npm not found\n'
    return
  fi

  npm list -g @openai/codex --depth=0 2>/dev/null \
    | sed -n 's/.*@openai\/codex@\([^ ]*\).*/\1/p' \
    | head -n 1
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --target-exe)
      target_exe=${2:?missing value for --target-exe}
      shift 2
      ;;
    --shim-dir)
      shim_dir=${2:?missing value for --shim-dir}
      shift 2
      ;;
    --install)
      install=1
      shift
      ;;
    --dry-run)
      dry_run=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [ -z "$target_exe" ]; then
  echo "--target-exe is required" >&2
  usage >&2
  exit 2
fi

target_path=$(abs_path "$target_exe")
shim_dir=$(abs_path "$shim_dir")
shim_path="$shim_dir/codex"
active_codex=$(command -v codex 2>/dev/null || printf 'not found on PATH')
npm_version=$(npm_codex_version)
[ -n "$npm_version" ] || npm_version="not installed or not discoverable"

echo "==> Codex++ shim self-check"
echo "Active codex path: $active_codex"
echo "Shim path: $shim_path"
echo "Target fork executable: $target_path"
if [ -f "$target_path" ]; then
  echo "Target reachable: true"
else
  echo "Target reachable: false"
fi
echo "Global npm @openai/codex version: $npm_version"

if [ "$dry_run" -eq 1 ] || [ "$install" -eq 0 ]; then
  echo "==> Dry run only; no files or PATH entries changed."
  exit 0
fi

if [ ! -f "$target_path" ]; then
  echo "Target fork executable does not exist: $target_path" >&2
  exit 1
fi

mkdir -p "$shim_dir"
{
  echo '#!/bin/sh'
  echo "exec $(shell_quote "$target_path") \"\$@\""
} > "$shim_path"
chmod +x "$shim_path"

echo "==> Installed shim at $shim_path"
if [ "$(command -v codex 2>/dev/null || true)" = "$shim_path" ]; then
  echo "==> Run: codex"
else
  echo "==> Run now: PATH=\"$shim_dir:\$PATH\" codex"
fi
