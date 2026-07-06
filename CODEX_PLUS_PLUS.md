# Codex++

This fork is upstream Codex plus local multi-account support.

It adds:

- `codex account import-current <label>` and `codex account list`
- account-scoped ChatGPT auth under shared `$CODEX_HOME/accounts`
- request routing that refreshes the selected account instead of root `auth.json`
- pre-stream usage-limit failover to the next eligible imported account
- small shims so the forked binary can sit before npm Codex on `PATH`

## Use

Build the fork:

```sh
cd codex-rs
cargo build --release -p codex-cli
```

Import accounts by logging into one account at a time, then:

```sh
codex account import-current work
codex account list
```

Install a shim:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\install\install-codex-plus-plus.ps1 -TargetExe .\codex-rs\target\release\codex.exe -Install -AddToUserPath
```

```sh
sh scripts/install/install-codex-plus-plus.sh --target-exe ./codex-rs/target/release/codex --install
```

The shim does not edit the npm package. It only points `codex` at the forked binary.

## Updates

If `codex update` or `npm install -g @openai/codex` installs upstream Codex, account data remains in `$CODEX_HOME/accounts`.

Rebuild the fork and rerun the shim installer. Multi-account support comes back.

For upstream pulls, keep this fork as a small patch stack on top of an OpenAI release tag. Do not store auth state in the npm package or a separate `CODEX_HOME`.
