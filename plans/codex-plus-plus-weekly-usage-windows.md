# Codex++ weekly usage-window auto-start

Investigation baseline: `d0554f85bce28d580d27e53b198aeca8b306a24e` (`v0.144.1` fork main)
WorkRequest: `wr_3bqpadctu3wa6pu6`
WorkPackage: `wp_rmzrzp34ujyiy33l`

## Decision

Use an isolated **in-process** Responses request. A background task owned by the embedded TUI
creates a dedicated `AuthManager` for each imported account home, uses the existing model-provider
and `codex-api` request stack, and never reaches the foreground app-server manager.

Reject a `codex exec` child process. The exec CLI has no account selector
(`codex-rs/exec/src/cli.rs:14`), so targeting an imported profile would require either rewriting the
root account marker or launching with a per-account `CODEX_HOME`. Both violate the product and
security constraints.

This is safe to implement after the two account/settings prerequisites land:

- The global weekly auto-start setting defaults to **enabled** and is toggled in
  `/codexplusplus`.
- Per-account eligibility reuses the account's existing **automation** toggle in `/accounts`.
  Do not add a second weekly-specific per-account toggle.
- The existing Codex++ welcome toast should then show only these two concise hints:
  `/codexplusplus` for fork settings and `/accounts` for account configuration.
- The pinned root main `auth.json` is immutable during scheduler work. The scheduler must never
  call `AccountStore::apply_imported_account_to_root_auth`.

The older WorkRequest copy saying the global feature defaults off and lives in `/accounts` is
superseded by these decisions.

## Verified current seams

### Account storage, authority, and locks

- `AccountProfile` and `AccountStore` are owned by `codex-login`
  (`codex-rs/login/src/account.rs:41`, `:78`). The index already records enabled,
  login-required, priority, and usage-reset metadata.
- `AccountStore::enabled_file_accounts` returns only enabled, login-ready file profiles
  (`account.rs:378`, with the filter at `:412`). This remains the base candidate source; the
  landed account-automation field becomes one additional filter.
- Import is the only path that copies the current root login into an imported profile and writes
  the root marker (`account.rs:93`). Foreground selection explicitly rewrites only that marker via
  `apply_imported_account_to_root_auth` (`codex-rs/tui/src/codex_plus_plus/startup_accounts.rs:184`).
  The scheduler does neither.
- Account index reads/writes are lock-protected and atomic (`account.rs:464`, `:468`, `:480`).
- `AccountLease` already supplies blocking and non-blocking OS file locks
  (`codex-rs/login/src/account_lease.rs:16`, `:30`). Dropping the file handle unlocks it (`:58`),
  so process death recovers a stale in-flight lease without a cleanup daemon.
- Foreground account-use leases are acquired when the active imported account changes
  (`codex-rs/login/src/auth/manager/codex_plus_plus/imported_account_selection.rs:198-204`).
  Scheduler dedupe must use a separate lease path; foreground use is not scheduler contention.

### Auth refresh and workspace restrictions

- Imported startup auth is filtered by enabled/login-ready state and the forced workspace set
  (`codex-rs/login/src/auth/manager/codex_plus_plus/imported_account_startup.rs:19-30`,
  `:102-126`).
- `AuthManager::new_with_automatic_account_selection` can construct an independent manager for an
  imported account home (`codex-rs/login/src/auth/manager.rs:1961`). Construct it with file
  storage, imported account home, the effective forced workspace IDs, the effective ChatGPT base
  URL/auth route, API-key env disabled, and automatic account selection disabled.
- `AuthManager::auth` performs proactive refresh (`manager.rs:2245`). Unauthorized recovery is the
  existing reload-then-refresh state machine (`manager.rs:1675`, `:1789`, `:2614`). Refresh holds
  the manager semaphore and the account-home `.auth-refresh.lock` (`manager.rs:2649-2663`).
- The expected account identity must be captured before the first request. The model-provider's
  `AuthManagerAuthProvider` follows refreshes for that identity but refuses an account/workspace
  change (`codex-rs/model-provider/src/auth.rs:124-151`, constructor at `:305`).

### Usage and request paths

- Startup already fetches all imported-account rate limits concurrently
  (`codex-rs/tui/src/account_usage.rs:68`). Its fetch path loads account-home auth, retries an
  unauthorized response through stored-auth refresh, and maps the weekly window
  (`account_usage.rs:122-170`, `:200-238`). Extend this owner instead of adding a second rate-limit
  client.
- The startup picker separately enforces forced workspaces before usage loading
  (`codex-rs/tui/src/codex_plus_plus/startup_accounts.rs:99-134`). The reusable scheduler fetch must
  accept the same workspace constraint directly because it runs outside the picker.
- `codex-api::ResponsesClient` supports a raw JSON HTTP stream (`codex-rs/codex-api/src/endpoint/responses.rs:26`,
  `:115`). The typed request deliberately has no `previous_response_id`; only the WebSocket shape
  exposes it and initializes it to `None` (`codex-rs/codex-api/src/common.rs:216-271`). Use HTTP
  only and create a fresh client per attempt.
- `codex-model-provider` already depends on `codex-api`, `codex-login`,
  `codex-model-provider-info`, and `codex-http-client`. It owns provider/auth construction
  (`codex-rs/model-provider/src/provider.rs:156-218`) and is the narrow owner for the one-shot ping.
- Provider URL/header/retry construction stays in `ModelProviderInfo::to_api_provider`
  (`codex-rs/model-provider-info/src/lib.rs:241`). Route-aware client construction comes from the
  effective config's `http_client_factory` (`codex-rs/core/src/config/mod.rs:1495`).
- The long-running embedded lifecycle starts the app server and then enters `App::run`
  (`codex-rs/tui/src/lib.rs:1332`, `:1396`, `:1836`; `codex-rs/tui/src/app.rs:759`). Start and own
  the scheduler there. Remote TUI sessions and `codex exec` do not host it.

## Minimal request contract

The model-provider helper issues one new HTTP `/responses` stream with no session or turn state:

```json
{
  "model": "<effective embedded TUI model>",
  "instructions": "",
  "input": [
    {
      "type": "message",
      "role": "user",
      "content": [{ "type": "input_text", "text": "Reply OK." }]
    }
  ],
  "tools": [],
  "tool_choice": "auto",
  "parallel_tool_calls": false,
  "store": false,
  "stream": true,
  "include": [],
  "max_output_tokens": 8
}
```

Rules:

- Use the effective model already resolved by the embedded TUI. Do not hard-code a model slug or
  discover models separately for every account.
- Use `ResponsesClient::stream`, not a WebSocket client and not a foreground `ModelClientSession`.
- Do not send `previous_response_id`, session/thread IDs, turn metadata/state, prompt cache keys,
  tools, history, reasoning configuration, service tier, or model-visible scheduler context.
- Treat the request as successful only after the stream reaches `ResponseEvent::Completed` **and**
  a fresh authoritative rate-limit fetch shows the weekly reset in the future. Ignore output text.
- On HTTP 401, run the existing `UnauthorizedRecovery` steps and rebuild request auth for each
  retry. Never switch accounts in this manager. Terminal refresh failures use
  `record_login_required_if_auth_matches` so a concurrent re-login is not overwritten.
- Bound the whole ping attempt with a 30-second timeout. Existing transport retries still handle
  transport/5xx failures; scheduler backoff handles the final failure.

## Eligibility and due-window predicate

Read account configuration fresh on every scan. An account is eligible only when all are true:

1. The global `/codexplusplus` weekly auto-start setting is enabled.
2. The imported profile is enabled and not login-required.
3. Its `/accounts` automation toggle is enabled.
4. Its auth file exists and passes the effective forced-workspace restriction.
5. Authoritative Codex usage includes a weekly/secondary window whose exact `used_percent` is
   `0.0`.
6. That weekly window has no reset timestamp or `resets_at <= now`.
7. The account is not inside scheduler backoff and its scheduler lease can be acquired.

Missing weekly usage is unknown, not unused. Any `used_percent > 0.0` is partial/running and must
not be pinged. Any `resets_at > now` is already running and must not be pinged, even if reported
usage is exactly zero.

`AccountUsage` currently stores rounded remaining percentages. Add an exact boolean derived from
the raw snapshot (`weekly_unused = used_percent == 0.0`); do not infer unused state from a displayed
`100%`, because rounding can hide partial use.

## Scheduler state and cross-process dedupe

Keep one non-secret fixed-shape state file and one OS lock beside each imported profile:

```text
accounts/<account-id>/weekly-window-state.json
accounts/<account-id>/weekly-window.lock
```

The state contains only:

```text
due_reset_at: null | unix-seconds
last_attempt_at: unix-seconds
failure_count: 0..8
retry_not_before: null | unix-seconds
last_success_reset_at: null | unix-seconds
last_error: null | transient | login_required | rejected
```

- `(account_id, due_reset_at)` is the attempt identity; `null` is a valid identity for a due window
  with no reset timestamp.
- Hold `weekly-window.lock` from the final due check through the post-ping usage verification and
  state write. Another process skips the account when `try_lock` reports contention.
- File locks are kernel-owned, so a crashed process releases the lease. The next five-minute scan
  is stale-lease recovery; file presence alone never means locked.
- On a new due identity, clear old failures. On failure, use
  `min(5 minutes * 2^failure_count, 6 hours)` and cap the counter at 8. The normal five-minute poll
  remains the only retry driver, so there is no busy loop.
- Persist only sanitized error categories. Detailed errors stay in trace/debug logs.
- Reject state input over 4 KiB, treat corrupt/unknown-version state as empty after a warning, and
  atomically replace through a same-directory temporary file. This state is reconstructable and
  never contains auth material.
- Read status through `AccountStore` so `/accounts` can show a concise retry/login error without
  parsing scheduler files in the TUI.

Sequential account processing is sufficient for the first version: typical account counts are
small, ordinary usage checks have a five-second timeout, and ping requests happen only for due
windows. Add bounded concurrency only if measured scan duration exceeds the five-minute cadence.

## File ownership and integration seams

Fork logic stays in capability-named `codex_plus_plus/` files. Existing upstream files receive
only declarations, exports, config/schema fields, match arms, and delegation calls.

### Slice 1: account state and request primitive

Target: at most 450 changed lines including focused tests.

- `codex-rs/login/src/account.rs`: module declaration/export seam only.
- `codex-rs/login/src/account/codex_plus_plus/mod.rs`: private module wiring.
- `codex-rs/login/src/account/codex_plus_plus/weekly_window_state.rs`: eligibility projection,
  per-account lease, bounded atomic state, status, and backoff.
- `codex-rs/login/src/account/codex_plus_plus/weekly_window_state_tests.rs`: state/lease tests.
- `codex-rs/login/src/lib.rs`: smallest required public re-export.
- `codex-rs/model-provider/src/codex_plus_plus/mod.rs`: module wiring.
- `codex-rs/model-provider/src/codex_plus_plus/weekly_window_ping.rs`: HTTP-only one-shot Responses
  helper using the existing provider/auth/transport stack.
- `codex-rs/model-provider/src/codex_plus_plus/weekly_window_ping_tests.rs`: wire body, auth identity,
  completion, unauthorized recovery, and timeout tests.
- `codex-rs/model-provider/src/lib.rs`: smallest required export.

This slice must rebase after the granular `/accounts` PR and use its landed automation field and
mutation API exactly. Do not introduce a parallel field.

### Slice 2: embedded TUI scheduler and UI integration

Target: at most 500 changed lines excluding intentional snapshots.

- `codex-rs/tui/src/codex_plus_plus/weekly_window_scheduler.rs`: five-minute loop, independent
  account managers, usage due check, ping orchestration, post-ping verification, and live enable
  watch handle.
- `codex-rs/tui/src/codex_plus_plus/weekly_window_scheduler_tests.rs`: paused-time scheduling,
  dedupe, eligibility, root-auth immutability, and foreground-session isolation.
- `codex-rs/tui/src/codex_plus_plus/mod.rs`: module/delegation seam only.
- `codex-rs/tui/src/account_usage.rs` and its existing sibling tests: accept forced workspace IDs
  in the reusable fetch and record the exact weekly-unused predicate.
- `codex-rs/tui/src/app.rs` or `lib.rs`: one scheduler construction/ownership delegation at the
  embedded lifecycle boundary. No loop or retry policy stays in the upstream file.
- The landed `/codexplusplus` handler updates the scheduler's `watch<bool>` only after the effective
  persisted value is known. Enabling triggers an immediate scan; disabling cancels no in-flight
  request but prevents the next account/scan.
- The landed `/accounts` view reads scheduler status from `AccountStore` and keeps its existing
  per-account automation workflow.
- Update the existing Codex++ welcome toast after both settings/account PRs are present; do not add
  a second toast or modify the generic upstream welcome help list.

The global setting should be represented in the fork settings config shape with a serde default of
`true`. If the prerequisite PR has not generated the core config schema for that field, run
`just write-config-schema` in the owning PR before this slice starts.

## Dependency and Bazel result

Verified with `cargo tree --depth 1` on all three owners:

```text
codex-tui -> codex-login
codex-tui -> codex-model-provider
codex-tui -> codex-backend-client
codex-model-provider -> codex-login
codex-model-provider -> codex-api
codex-model-provider -> codex-http-client
codex-login -> codex-config/codex-protocol/codex-http-client
```

No new crate dependency is required, so the proposal cannot introduce a Cargo cycle. In
particular, login and model-provider never depend on TUI.

No proposed code uses `include_str!`, `include_bytes!`, `sqlx::migrate!`, or another compile-time
file read. The state JSON is runtime data. `codex-rs/tui/BUILD.bazel` already globs crate data
(`:5-23`), while login and model-provider use the standard `codex_rust_crate` source discovery
(`codex-rs/login/BUILD.bazel:3`, `codex-rs/model-provider/BUILD.bazel:3`). No `compile_data` or
`build_script_data` addition is needed.

## Required tests and gates

### Slice 1

- `just test -p codex-login`
- `just test -p codex-model-provider`
- `just fix -p codex-login`
- `just fix -p codex-model-provider`
- Unit tests prove lease contention/drop recovery, due-identity reset, capped backoff, atomic
  bounded state, corruption recovery, and sanitized status.
- Wire tests prove the exact request has no tools/history/session IDs/previous response/WebSocket,
  refresh never crosses identity, and only `Completed` counts as request completion.

### Slice 2

- `just test -p codex-tui`
- `just fix -p codex-tui`
- Snapshot `/codexplusplus`, `/accounts` scheduler status, and the Codex++ welcome toast.
- A two-scheduler test proves one due account produces one ping.
- A root-auth test compares the root `auth.json` bytes before and after a successful ping and a
  terminal refresh failure.
- A foreground-isolation test proves the active account ID, request history, thread/session state,
  WebSocket state, and model-visible context are unchanged.
- A workspace test proves an out-of-policy imported account is skipped before any ping.
- Paused-time tests prove immediate first/enabled scan, five-minute cadence, disabled silence,
  exponential retry eligibility, and no busy loop.

Both slices run `just fmt`, `git diff --check`, pending-snapshot inspection where applicable, and
Review Suite `deep`. Because these changes do not alter app-server APIs, no app-server schema or
protocol documentation changes are required. A full workspace `just test` still requires explicit
user approval.

## Stop / recut triggers

Stop rather than broadening if implementation discovers any of these:

- A Responses request cannot authenticate from an imported account-home `AuthManager` without
  copying tokens or rewriting the root marker.
- The server requires foreground thread/session/WebSocket state or app-server protocol changes.
- The granular account PR does not expose an automation eligibility bit without token snapshots.
- The global settings PR cannot provide the effective persisted toggle to the live scheduler.
- The proposed helper needs a TUI dependency from login/model-provider or a new crate solely for
  fork isolation.

Do not add an OS task, tray process, daemon, service, per-account `CODEX_HOME`, token snapshot,
app-server RPC, or root-auth restore path in the first version.
