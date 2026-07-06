# Codex++ install wrapper

Use `scripts/install/install-codex-plus-plus.ps1` to put a user-owned
PowerShell `codex.ps1` shim ahead of the npm-installed `codex` command on
`PATH`.

Dry-run/self-check:

```powershell
pwsh -File scripts/install/install-codex-plus-plus.ps1 `
  -TargetExe C:\path\to\fork\codex.exe `
  -DryRun
```

Install:

```powershell
pwsh -File scripts/install/install-codex-plus-plus.ps1 `
  -TargetExe C:\path\to\fork\codex.exe `
  -Install `
  -AddToUserPath
```

The durable mechanism is the shim directory, not edits inside the global
`@openai/codex` npm package. Installing or updating upstream Codex through npm
can replace npm-owned files, but restoring Codex++ is just rerunning this script
or ensuring the shim directory stays earlier on `PATH`.

On Windows, machine `PATH` entries can outrank user `PATH` entries. When
`-AddToUserPath` is used, the script verifies the future command winner and
fails if a machine-wide Codex still shadows the shim.
Relative `-ShimDir` values are resolved to absolute paths before writing the
shim or changing `PATH`.
When `-Install` is used without `-AddToUserPath`, it only prints `Run: codex`
after verifying that the current shell resolves the exact `codex.ps1` shim
first. If a PATH prepend still would not select that shim, it prints a direct
shim invocation instead.

The durable shim is PowerShell-native so Unicode profile paths and target paths
do not round-trip through `cmd.exe` code page expansion.
It forwards pipeline input to the forked executable, preserving stdin-driven
commands such as `codex exec -`.

The self-check also reports the future PowerShell execution policy, including
the default restricted policy when persistent scopes are undefined, and warns
when a normal shell may block the local `codex.ps1` shim.

The wrapper does not read, write, move, or delete `CODEX_HOME`, `auth.json`, or
`CODEX_HOME\accounts\**`. Account-scoped auth state remains under the shared
Codex home and survives upstream npm installs.

For a one-off smoke only, replacing files inside the npm package can prove an
artifact launches, but it is intentionally not the install path because npm can
overwrite it and package mutation risks confusing ownership.

Release tags should be fork-owned and upstream-version-compatible, for example
`codexpp-v0.142.5.1` for a fork based on upstream `rust-v0.142.5`. Prefer visible
binary metadata such as `0.142.5+codexpp.1` when the packaging path accepts it.
If update prompts are controlled by npm package metadata or OpenAI's remote
update service, document that blocker instead of spoofing update state or
patching the update protocol.
