# Codex++

This is upstream Codex with one local extra: multi-account CLI auth.

It keeps the normal `$CODEX_HOME` for config, sqlite, sessions, and plugins, and stores imported account auth under `$CODEX_HOME/accounts`. When one imported account hits a usage limit, Codex++ tries the next enabled imported account.

Enable it:

```sh
cd codex-rs
cargo build --release -p codex-cli
codex account import-current main
codex account list
```

Use the forked binary with the PATH shim:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\install\install-codex-plus-plus.ps1 -TargetExe .\codex-rs\target\release\codex.exe -Install -AddToUserPath
```

```sh
sh scripts/install/install-codex-plus-plus.sh --target-exe ./codex-rs/target/release/codex --install
```

Disable it by removing the shim from PATH or putting upstream Codex earlier on PATH. Disable a single account by setting `"enabled": false` in `$CODEX_HOME/accounts/index.json`.

If an upstream Codex update replaces the binary, rebuild this fork and rerun the shim. Your account store stays in `$CODEX_HOME/accounts`.

Happy Codex-ing.
