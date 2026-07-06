#!/bin/sh
set -eu

target_exe=
shim_dir="${HOME:-.}/.local/bin"
install=0

usage() {
  cat <<'EOF'
Usage: install-codex-plus-plus.sh --target-exe PATH [--shim-dir DIR] [--install]
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

echo "==> Codex++ shim"
echo "Active codex path: $active_codex"
echo "Shim path: $shim_path"
echo "Target fork executable: $target_path"
if [ -f "$target_path" ]; then
  echo "Target reachable: true"
else
  echo "Target reachable: false"
fi

if [ "$install" -eq 0 ]; then
  echo "==> Dry run only; pass --install to write the shim."
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
