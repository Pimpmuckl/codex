# Codex++

This is upstream Codex with one local extra: multi-account CLI auth.

It keeps the normal `$CODEX_HOME` for config, sqlite, sessions, and plugins, and stores imported account auth under `$CODEX_HOME/accounts`. When one imported account hits a usage limit, Codex++ tries the next enabled imported account.

With MultiAgentV2, `spawn_agent` also accepts `cwd`. Relative paths are resolved against the current agent's working directory, so subagents can start in a different folder without changing the parent session.

Enable it:

```sh
cd codex-rs
cargo build --release -p codex-cli
codex account add
codex account list
```

Build a package with a temporary fork version such as `<upstream-version>-fork`:

```sh
python scripts/build_codex_plus_plus.py -- --package-dir dist/codex-plus-plus --force
```

The script restores `codex-rs/Cargo.toml` and `codex-rs/Cargo.lock` after the build and prints the matching fork tag name.

Use the forked binary with the PATH shim:

```powershell
.\scripts\install\install-codex-plus-plus.ps1 -TargetExe .\codex-rs\target\release\codex.exe -Install -AddToUserPath
```

```sh
sh scripts/install/install-codex-plus-plus.sh --target-exe ./codex-rs/target/release/codex --install
```

Disable the forked binary again:

```powershell
.\scripts\install\install-codex-plus-plus.ps1 -Remove
```

```sh
sh scripts/install/install-codex-plus-plus.sh --remove
```

If an upstream Codex update replaces the binary, rebuild this fork and rerun the shim. Your account store stays in `$CODEX_HOME/accounts`.

Happy Codex-ing.
