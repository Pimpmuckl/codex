# Codex++ main-account credential lifecycle

Status: implementation-ready investigation
Evidence base: `f9e94a158ed7a56646f89d84ea031d02b1cff070` (`origin/main` at final verification)

## Decision

Codex++ may pin one imported ChatGPT account as the root `CODEX_HOME` authority used by an
unmodified desktop or other non-fork client. Codex++ continues to select and rotate accounts
internally without rewriting root auth on every selection.

The first implementation supports pinning only while `cli_auth_credentials_store = "file"`.
Pin, repin, unpin, startup, refresh, status, and replacement must reject `keyring`, `auto`, and
`ephemeral` before reading or writing pinned root auth. Changing the setting after pinning fails
closed until it is restored to file mode or the account is explicitly reconciled by a file-mode
fork:

- file mode can move credentials between root, profiles, and a staging slot with same-filesystem
  renames; a refresh token is never copied;
- keyring and auto provide no atomic move between the root secret and profile files, and auto may
  later expose a stale keyring value after a file fallback;
- ephemeral root auth is process-local, cannot serve the desktop, and moving a durable profile
  credential into it would destroy that credential at process exit.

No desktop change, app-server wire change, dependency, new crate, or second refresh-token
authority is required.

## Supported concurrency contract

Sequential use is supported: pair the desktop/mobile account, close or stop credential mutation
there, then use Codex++ with that account pinned. Simultaneous refresh, login, logout, or root
replacement by an unmodified client is not safe because that client does not acquire Codex++
locks.

Codex++ guarantees that its own processes serialize root/profile transitions and refreshes. It
cannot guarantee that an unmodified client will not consume the same one-time refresh token,
truncate `auth.json`, replace it with another identity, or delete it concurrently. Fork mutations
therefore compare the root identity and credential fingerprint with their precondition and fail
closed on drift. They never overwrite either side after drift is observed.

A same-identity root refresh performed earlier by the desktop is accepted on the next quiescent
Codex++ startup by updating metadata only. A different identity, missing root, malformed root, or
root change during a transition remains blocked until the user explicitly reconciles it. If an
unmodified client destroyed the only root credential, Codex++ cannot reconstruct it; re-login is
required.

The reverse handoff has one explicit gate because an unmodified client cannot recover fork-owned
pending files: before reopening the desktop after Codex++ has used the pinned root, Codex++ must
complete its recovery pass. Normal fork shutdown and the next clean fork startup do this; after an
abnormal exit, the user runs `codex account reconcile-main` before launching the desktop. That pass
promotes any recoverable pending refresh and verifies root/lifecycle fingerprints. Codex++ cannot
make an unmodified client enforce this gate, so opening it first after an abnormal fork exit is
outside the supported contract.

Pinning preserves the account's existing `automation_enabled` value. The main account remains
eligible for automatic selection unless the user explicitly disables it in `/accounts`.

## Current lifecycle map

### Storage and authority

| Surface | Current-main evidence | Current behavior and implication |
| --- | --- | --- |
| Root auth payload | `codex-rs/login/src/auth/storage.rs:38-60` (`AuthDotJson`) | A full ChatGPT payload contains the access and refresh token. |
| File storage | `storage.rs:191-223` (`FileAuthStorage`) | `save` opens root with `truncate(true)` and writes in place. A crash can leave an empty or partial authority. |
| Direct/secrets keyring | `storage.rs:291-318`, `354-401` | Saves one serialized root secret, then best-effort removes file fallbacks; no cross-backend transaction exists. |
| Auto | `storage.rs:427-452` | Reads keyring first and falls back to file on absence/error; saves to file after a keyring error. It cannot prove which backend is authoritative across failures. |
| Ephemeral | `storage.rs:455-495` | A process-global map keyed by home; it is not durable or visible to another process. |
| Backend selection | `storage.rs:498-539` | Root may use all four modes. Imported profiles currently always use file mode. |
| Shared raw helpers | `auth/manager.rs:1063-1094` | `save_auth` and `load_auth_dot_json` bypass `AuthManager` policy and directly address a backend. |

### Account import, selection, and refresh

| Entry point | Current-main evidence | Current behavior |
| --- | --- | --- |
| Import root login | `codex-rs/login/src/account.rs:99-238` | Copies full root auth to `accounts/<id>/auth.json`, commits index, then replaces root with a marker whose refresh token is cleared. Rollback copies payloads again. |
| Root marker | `account.rs:629-648` | Marker detection is only “managed ChatGPT auth with an empty refresh token.” |
| Explicit startup selection | `codex-rs/tui/src/codex_plus_plus/startup_accounts.rs:170-194` | Every user or timed selection calls `apply_imported_account_to_root_auth`, so selection rewrites root identity. |
| Apply selected account | `account.rs:331-369` | Loads the profile's full file auth but writes only a marker to root. |
| Runtime startup | `codex-rs/login/src/auth/manager.rs:1935-2061` | Loads root first, then optionally chooses a profile; if profile selection returns none, generic root auth is the fallback. |
| Imported-account load | `auth/manager/codex_plus_plus/imported_account_startup.rs:19-90,102-124` | Enumerates enabled profile files and always loads them in file mode. |
| Manual/failover activation | `auth/manager/codex_plus_plus/imported_account_selection.rs:74-115,128-211` | Changes the active auth home to a profile and refreshes that profile in place. |
| Refresh persistence | `auth/manager.rs:2646-2752,2894-2912` | Acquires fork refresh locks, reloads the active source, refreshes, then saves through that source's backend. |
| Terminal refresh failure | `auth/manager/codex_plus_plus/imported_account_refresh.rs:178-243` | Marks the profile login-required and may switch away. |
| Logout | `imported_account_refresh.rs:113-175,246-263` and `auth/manager.rs:2755-2789` | Fork logout revokes/destroys root and every imported profile, then disables all accounts. |

The generic root fallback is a policy bypass once root contains a pinned full credential. If the
pinned profile is excluded from automatic selection, `AuthManager` must not silently use raw root
after the managed-source selector returns no candidate.

### Login and replacement writers

All managed login writers eventually target the supplied `codex_home`:

- browser OAuth and device code persist through
  `codex-rs/login/src/server.rs:860-903` (`persist_tokens_async`);
- direct CLI browser login clears/revokes existing root before opening OAuth at
  `codex-rs/cli/src/login.rs:120-165`;
- direct CLI device and device-fallback login repeat that destructive pre-clear at
  `cli/src/login.rs:306-349,351-423`;
- API-key and access-token/PAT/agent-identity login write root directly at
  `cli/src/login.rs:199-263` through `auth/manager.rs:960-1033`;
- `codex account add` runs the same root browser login and only afterward imports it at
  `codex-rs/cli/src/account_cmd.rs:37-75`;
- app-server API-key login writes root at
  `codex-rs/app-server/src/request_processors/account_processor.rs:306-357`;
- app-server browser and device-code login pass root to the same login server at
  `account_processor.rs:359-577`;
- app-server external ChatGPT tokens are process-local, not a persistent root replacement, at
  `account_processor.rs:608-680` and `auth/manager.rs:2858-2877`;
- app-server logout calls the same all-managed-auth path at
  `account_processor.rs:755-807`.

CLI status and doctor also read raw root (`cli/src/login.rs:425-479`,
`cli/src/doctor.rs:1212,2586`). They must report the pinned root as root auth, but diagnostics must
not use raw root as permission to bypass account automation policy.

## Rejected recut

The non-main attempt in `9b1ca9dbfb` plus `5ca64714c5` correctly introduced a main-account id and
root-backed account source, but it must not be revived as-is:

1. It used `save_auth` to copy previous root into a profile, copy target into root, and only then
   delete target. That creates duplicate persistent full-token authorities.
2. Its journal was recoverable only after successful full-payload writes; file truncation can
   destroy the source before recovery can identify it.
3. It attempted the same algorithm for file, keyring, auto, and ephemeral root modes even though
   only file mode has a same-filesystem move primitive.
4. `AuthManager` could still fall back to the raw pinned root when managed automatic selection
   excluded that account.
5. Re-importing a main account skipped the profile write without validating that replacement root
   identity was the pinned identity.
6. Direct CLI and app-server login writers still replaced or pre-cleared root outside the journal.

## Target state

New lifecycle metadata lives in a separate fork-owned
`accounts/codex-plus-plus-lifecycle.json`, not `accounts/index.json`:

```text
main_account_id: Option<AccountId>
main_root_fingerprint: Option<RootFingerprint>
pending_auth_transition: Option<AuthTransition>
automatic_selection_suspended: bool
```

Existing/pre-upgrade index writers never open this file, so their ordinary usage, priority, or
login-required saves cannot discard a pending transition after a crash. The lifecycle journal
covers coordinated updates to this file and the existing account index; it remains present until
both final states are durable. Slice 1 adds a strict lifecycle-only index commit helper that syncs
its temporary file, atomically replaces, and syncs the accounts directory on POSIX; ordinary
non-lifecycle `AccountStore::save_index` remains available on every currently supported filesystem.
On Windows lifecycle transactions, where directory `FlushFileBuffers` is not supported, require
NTFS and open every same-directory temporary with
`FILE_FLAG_WRITE_THROUGH`; flush its contents, then install it through
`SetFileInformationByHandle(FileRenameInfo)` on that still-open handle and reopen it to verify the
expected fingerprint. The [CreateFileW write-through
contract](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew)
explicitly includes NTFS metadata changes such as rename; do not infer durability from
`MOVEFILE_WRITE_THROUGH`. Persist the lifecycle journal by that protocol before credential
mutation, commit the index by that protocol after credential mutation, and only then replace the
lifecycle file with its cleared state by the same protocol. A returned index commit therefore
precedes journal clearing on both platform paths; startup treats any verification failure as a
still-pending transition and reconciles before reading the index. The generic `FileAuthStorage`
replacement below uses the verified Windows path when a lifecycle operation has passed the NTFS
gate; its ordinary non-lifecycle fallback remains usable elsewhere rather than widening the gate to
all login/account writes. New writers read lifecycle state before interpreting root or profile
auth. Absence of the file means the backward-compatible no-main/default state.

`RootFingerprint` contains the derived account id plus a SHA-256 digest over the serialized token
identity/generation fields. It contains no bearer or refresh token. The existing account profile
remains the user-facing record; `main_account_id` changes where its credential is resolved. There
is no second profile `auth.json` for the main account.

Atomic file storage uses a separate `StorageFingerprint`: SHA-256 over the stable serialized full
`AuthDotJson`, including its auth-mode discriminant. It does not require a ChatGPT account id and
therefore covers API-key, PAT, agent-identity, Bedrock, and ChatGPT payloads. `RootFingerprint`
remains the policy identity/generation check only for a pinned ChatGPT main.

Malformed-root reconciliation uses a third precondition, `RawFileFingerprint`: the exact byte
length plus SHA-256 of at most 1 MiB read from the already-open no-follow file handle. Journal only
that length/digest, move the file to quarantine without replacement, then re-read the moved handle
and require the same fingerprint. A larger or unreadable malformed root is not auto-quarantined and
requires manual recovery; malformed bytes are never parsed or copied into lifecycle metadata.

`automation_enabled` now exists with default `true` at `account.rs:50` and is updated through the
fork-owned `codex_plus_plus/account_policy.rs:7-27`. Pin, repin, unpin, re-import, and drift
reconciliation never change it. Existing indexes start with no lifecycle file,
`automatic_selection_suspended = false`, and retain current behavior until the user pins one.

The fork-owned implementation lives under the nearest owner:

```text
codex-rs/login/src/codex_plus_plus/main_account.rs
codex-rs/login/src/codex_plus_plus/main_account_tests.rs
codex-rs/cli/src/codex_plus_plus/account_login.rs
codex-rs/app-server/src/request_processors/account_processor/codex_plus_plus/main_account_login.rs
```

`account.rs`, `auth/manager.rs`, CLI dispatch, and app-server dispatch keep only serialized fields,
small delegation calls, and unavoidable match arms. Do not add a new crate or broadly expose
internals. Export only a focused lifecycle facade from `codex-login`—prefer narrow methods on the
existing public `AccountStore` plus opaque request/result types—so CLI and app-server can delegate
without duplicating transaction logic.

## File-mode transaction

The lifecycle journal is stored atomically inside `accounts/codex-plus-plus-lifecycle.json`. Every credential move is
a no-clobber rename within the same `CODEX_HOME`; auth contents are never copied. Implement one
private `move_noreplace` primitive with `renameat2(RENAME_NOREPLACE)` on Linux,
`renameatx_np(RENAME_EXCL)` on macOS, and
`SetFileInformationByHandle(FileRenameInfo)` with `ReplaceIfExists = FALSE` on Windows. Unix calls
use already-open source/destination parent directory file descriptors. Windows requires an NTFS
volume, opens the source with `FILE_FLAG_WRITE_THROUGH` and no-follow/reparse checks, uses only fixed
validated lifecycle paths, and revalidates the destination afterward. Destination existence is
always an error; never fall back to ordinary `rename` or replacement. A
transaction-specific staging directory under `accounts/.auth-transitions/<id>/` holds a moved
slot, not an additional copy. The journal stores only an operation kind, fixed-format operation
id, typed account ids, expected identities/fingerprints, intended final main id, index precondition
fingerprint, and bounded post-operation `AccountProfile` images for only the affected ids. Those
post-images preserve caller-supplied labels, priorities, and policy flags across recovery; they
contain no credentials. The journal never stores arbitrary paths or auth payloads. Recovery derives
the only permitted root, profile, and transition-slot paths.
Account ids must match exactly `acct_[0-9a-f]{16}` (the current constructor at `account.rs:602`);
operation ids use a separate fixed lowercase-hex grammar. Reject invalid deserialized ids before
any filesystem access, so `..`, separators, alternate prefixes, and absolute paths cannot escape
lifecycle-owned directories. Resolve lifecycle parents without following links: reject symlink or
Windows reparse/junction components and open through no-follow platform primitives. POSIX reads and
moves are relative to retained trusted handles. The root slot is only the fixed
`<canonical CODEX_HOME>/auth.json` under the canonical `CODEX_HOME` handle; profiles, staging, and
quarantine must remain beneath the separate owned canonical `CODEX_HOME/accounts` handle. Reject a
symlink/reparse-point credential leaf before and after every move and fingerprint reload; never move
or deserialize through a followed `auth.json` link.

Before writing the journal, acquire refresh locks for root and affected profiles in sorted-path
order, then the index lock. Recovery first snapshots the operation id and derived lock set, acquires
those locks in the same order, reloads the journal and derived paths under the index lock, and
releases/retries if either changed. Validate all identities, fingerprints, source existence,
destination absence, store mode, and absence of another transaction. Open and sync every validated source file
and sync its current parent directory before persisting the journal; this makes even a file written
by an older/unmodified client durable before its directory entry moves. Persist and sync the
journal only after confirming every source/destination parent has the same filesystem/volume
identity and the platform no-clobber primitive is available. A cross-device/bind-mount layout is
rejected before journal creation. If journal persistence succeeds but the first move is proven not
to have occurred, recovery may safely clear that no-op journal after revalidating all pre-state
fingerprints. After every move, sync both the source and destination parent directories on POSIX.
On Windows, sync the source file first, rename through the source handle opened with
`FILE_FLAG_WRITE_THROUGH`, and reject non-NTFS filesystems before journal creation. This makes the
documented write-through metadata behavior—not `MoveFileExW`'s copy-and-delete-only flush
semantics—the durability primitive. Retain the journal through post-move destination
identity/fingerprint validation and the next durable lifecycle commit. Do not require unsupported
directory `FlushFileBuffers`. Reload and compare every affected slot after each move and
immediately before the next move and final index commit.
After moving a source into the transaction slot, require its fingerprint to equal the journal precondition before
installing another credential. Recovery inspects actual file identities rather than trusting a
phase counter and only completes the declared transition forward. Any occupied destination or
extra, missing, malformed, or changed credential fails closed with every discovered file left in
place.

A blocked journal is recoverable only through an explicit journal-aware reconcile action. Under the
same snapshot/recheck lock loop, it classifies every root/profile/transaction credential by identity
and fingerprint, then offers two non-destructive choices when feasible: finish the declared
transition, or restore its recorded pre-state. Any unexpected valid credential is first moved
without replacement to a lifecycle-owned conflict quarantine and retained for explicit import;
unknown/corrupt material is quarantined, never overwritten. The journal is cleared only after the
chosen placement and index metadata are fully committed. If either reconstruction lacks required
material, reconciliation remains blocked and reports the exact missing slot instead of guessing.
Expose this as `codex account reconcile-main` and as a `/accounts` repair row whenever a pending or
drifted lifecycle exists. The UI shows account labels and final-placement choices, never tokens.
Slice 3 must wire finish/restore/cancel actions and snapshots before lifecycle code is shippable; no
new app-server RPC is required.

Repin `A -> B`:

1. persist journal expecting full `A` at root, no profile auth for `A`, and full `B` at its profile;
2. move root to the empty transaction slot without replacement, then validate the moved fingerprint;
3. move `B` profile auth to root without replacement (a concurrently recreated root makes this fail);
4. revalidate root as `B`, then move the transaction slot to the empty `A` profile path without replacement;
5. revalidate root `B`, profile `A`, and empty profile `B`, then atomically commit
   `main_account_id = B`, the new root fingerprint, and no journal.

At every boundary each full credential exists exactly once. Initial pin is the same operation with
a root marker or empty root; the old marker is moved to the slot and removed after target reaches
root. Unpin moves root to the main profile, atomically writes a marker to root, and clears main
metadata. Selecting an account for a session never invokes any of these operations.

Initial pin with an occupied full root has explicit preconditions. If root is the target identity,
pin it in place after moving duplicate target-profile auth to an inactive tombstone and
removing that old copy. Full storage fingerprints remain the transition preconditions, but they do
not identify revocation authority, and unequal refresh-token strings may still be generations of
one provider-side grant. If another credential for the same ChatGPT account is retained, delete the
superseded credential locally without revocation unless the provider exposes a stable grant id and
the two grants are proven distinct; current token payloads provide no such proof. Revoke only when
no credential anywhere in the intended final root/profile/staging state has the superseded
credential's account id (for example explicit logout). Evaluate each tombstone against its own
account id, not against the replacement account. Apply this rule to every replacement tombstone.
Never persist raw tokens or a derived token comparison in metadata. If root is a
different identity, reject pin before journaling and instruct the user to run
`codex account import-current`; that transaction safely rehomes root to its profile and leaves a
marker, after which pin can proceed. Never overwrite or silently discard an occupied full root.

Root markers are prepared as synced `0o600` temporary files, then installed only with
`move_noreplace`; they never use replacement-style `FileAuthStorage::save`. If an unmodified client
recreates root after the full credential moved, marker installation reports drift and leaves that
new root plus the journaled moved credential untouched for recovery.

The generic `FileAuthStorage` must first gain a recoverable atomic-save protocol in an isolated
upstreamable commit. Under the auth refresh lock, create one discoverable pending file in the same
directory with Unix mode `0o600`. Its filename encodes fixed-format prior/new fingerprints, while
its contents are exactly the new `AuthDotJson` wire shape; no envelope is ever installed as
`auth.json`. Write and sync that file, sync its parent, atomically replace `auth.json` with it, then
sync the parent again. Before ordinary load returns an older `auth.json`, recover a pending file
under that same lock: parse its filename and contents, promote it only when its identity plus
prior/new fingerprint preconditions match, otherwise fail closed. Split locked/unlocked storage
helpers so callers already holding the refresh lock do not reacquire it. This preserves a newly
rotated one-time refresh token if the process dies after syncing the pending file but before
replacement. Preserve or tighten root/profile permissions. Delete/logout under the same lock first
snapshots any pending raw auth for revocation, then creates a no-replace, non-secret deletion-intent
marker containing only their fingerprints. Sync the marker and parent on POSIX; create it through a
write-through handle on Windows. Only after that durable intent exists, move each pending file and
`auth.json` without replacement into ignored tombstones, syncing the parent after each POSIX move or
using the Windows write-through-handle rename. Ordinary load checks the intent and tombstones before
returning auth and completes the declared deletion under the same lock. Save and every lifecycle
credential-install path run that same recovery before creating a pending file or moving fresh auth,
so a stale intent can never target a later login. Best-effort physical cleanup removes tombstones,
syncs the parent, then removes and syncs the intent marker; a marker or tombstone resurrected by
power loss remains outside every auth loader and is cleaned by recovery. A later load therefore
cannot resurrect or wedge on a deletion that reported success.

Any lifecycle transition that creates an inactive credential slot commits its intended main/index
state with `pending_auth_transition = CleanupPending` instead of clearing the journal. Before that
commit, create the durable deletion intent for every inactive slot. Recovery retries idempotent
revocation when the authority rule requires it until the call succeeds or the server reports the
token already invalid; same-account replacement skips revocation and proceeds directly to durable
local removal. Only after every required revocation/removal is complete may it clear the lifecycle
journal. Logical logout can report that the account is inactive while offline cleanup is pending,
but `/accounts` and `reconcile-main` expose that state rather than forgetting the live tombstone.

Add save/load/delete crash-boundary and Unix permission regression tests. Lifecycle slot changes
still use no-clobber moves because atomic save alone would copy a refresh token.

## Login staging and lifecycle rules

Managed browser/device login must target an operation-owned staging home under `accounts`, always
using file storage. It is temporary and never becomes a persistent per-account `CODEX_HOME`.
Hold an operation-specific cross-process lease from before staging creation until commit or normal
cleanup. Normal completion moves its `auth.json` exactly once into root or a profile. Cancellation
acquires the operation lease, inspects the staging slot and retained root/profiles, applies the same
revocation-authority rule, durably removes staged auth through the storage delete protocol, and only
then removes the manifest. A staged credential for an account that remains active is deleted
without revocation even when its refresh-token string differs. A small non-secret manifest lets
next startup perform the same cleanup sequence for an orphan or resume a committed transition after
process death. Orphan cleanup must first acquire that operation lease nonblockingly; an active lease
means the login is live and must be left untouched.

- `codex account add`: stage login, validate ChatGPT/workspace/account identity, then rename into
  an empty profile. If it reauthenticates the main id, use root replacement below. If a non-main
  profile already exists, journal `ReplaceProfile`: move old profile to the transaction slot,
  validate it, move staged auth to the now-empty profile without replacement, revalidate, commit
  index metadata with cleanup pending, then revoke/remove the inactive old slot and clear the
  journal. If metadata exists but its auth file is
  absent/login-required, journal `InstallProfile`, move staged auth directly into the empty profile,
  then clear login-required and automatic-selection suspension in the final index commit. It never
  clears an unrelated root. If staged identity equals an existing full unpinned root, journal
  `ImportStagedSameRoot`: move old root to an inactive tombstone, install staged auth directly as
  the profile authority, install a root marker without replacement, commit index metadata with
  cleanup pending, then revoke/delete the old tombstone and clear the journal. This prevents
  root/profile duplicate refresh authorities.
- `codex account import-current`: with no main, replace the current copy-then-marker sequence with
  a journaled move when root is full. If the target profile is empty, move root directly into it. If
  the profile already holds auth, journal `ReplaceProfileFromRoot`, move that old profile into the
  transaction slot, validate it, then move root into the now-empty profile without replacement.
  Install the root marker, commit index metadata with cleanup pending, then revoke/delete the
  inactive old slot and clear the journal. When
  root is already a legacy marker, load and validate the existing full profile
  of the same account and treat import as an idempotent metadata update; never try to move the
  marker as the full credential. Missing or mismatched marker-backed profile auth fails closed. With
  a main, require root identity to equal that main and treat the command as an idempotent
  metadata/re-login-required update; never copy root into its profile or replace root with a
  marker. A different root identity is drift and fails closed.
- Direct browser/device `codex login`: a pinned root is necessarily file-backed and always stages;
  that path never calls `clear_existing_auth_before_login`. With no main and file storage, stage and
  journal replacement of old root only after successful authentication. If the staged identity
  already has an imported profile, use `ReplaceRootRetireProfile`: move old root and that profile
  into distinct inactive slots, install staged auth at root, mark the matching profile
  login-required while preserving its label/policy metadata, then clean both old slots under their
  respective revocation dispositions. This leaves one durable authority; a later `account add` can
  re-import that root through the existing same-root transaction. With no main and
  keyring/auto/ephemeral storage, leave the existing upstream login/revocation path unchanged; it is
  outside the file-only main lifecycle, and this work must not invent a partial cross-backend
  transaction. The no-pre-clear guarantee applies to file-backed/pinned transitions only.
  With a main, the same main identity atomically replaces root; a different identity is rejected
  without changing root and instructs the user to add it then choose it as main.
- API-key, PAT, agent-identity, and Bedrock root replacement: reject while a main is pinned; the
  user must unpin first. External ephemeral ChatGPT tokens remain process-local and do not change
  main metadata.
- Fork app-server browser/device login: use identical staging and validation internally; the JSON-
  RPC protocol is unchanged. Persistent API-key replacement follows the same pinned-main guard.
  `LoginAccountParams::ChatgptAuthTokens` remains process-local through `set_external_auth`, never
  writes root, and is explicitly exempt from that guard.
- Root replacement for the same main: move old root to a transaction slot, move validated staged
  auth to root, commit the new fingerprint with cleanup pending, then revoke and remove the inactive
  old slot before clearing the journal. A crash resumes cleanup without making the old slot an
  active source.
- App-server logout while `ChatgptAuthTokens` external auth is active clears only that process-local
  state and returns; it does not journal, revoke, move, or relabel any durable managed credential.
  Once external auth is absent, logout follows the managed active-source rule below.
- Explicit logout: journal `LogoutActive` before revocation, credential movement, or index changes.
  Move the active root/profile credential without replacement into an inactive transaction
  tombstone and validate it, then atomically commit `automatic_selection_suspended = true` plus the
  main-clear or profile-login-required metadata with cleanup pending. Recovery retains that intent
  until revocation and durable tombstone deletion complete, so a crash cannot leave an active
  credential, forgotten live tombstone, or automatic reselection after a reported logout. Logging out the main
  clears root and main metadata but leaves unrelated profiles and their automation settings intact.
  Logging out an alternate marks only that profile login-required. A successful explicit account
  selection or login clears the suspension. No login flow calls logout as a preparatory step. A
  future `logout-all` command is out of scope.
- Remove main account: reject until it is unpinned or another main is selected. Disable main
  automation: keep root and the main designation; exclude it only from fork automatic selection.
- Missing/corrupt main root: fail closed and require re-login. Never restore from a hidden profile
  copy because no such copy may exist. A successful staged same-main re-login may start an explicit
  user-authorized reconciliation: `RestoreMissingMain` installs into an expected-empty root;
  `ReplaceCorruptMain` first moves the malformed root without replacement into a quarantine slot,
  using the journaled bounded `RawFileFingerprint` as its before/after precondition, installs the
  staged credential, commits the verified fingerprint, then deletes the quarantine. Neither
  reconciliation runs automatically, and any root recreated during it blocks installation.
- Forced workspace mismatch: fail before journal creation for new operations. If configuration
  later excludes the pinned root, keep credentials untouched and expose unauthenticated state.

## Runtime selection changes

Account enumeration returns a typed managed source: profile file or pinned root. Root is a managed
source whenever `main_account_id` is set. It is never reconsidered by the generic root fallback.

The session retains its explicitly activated logical account id, not a physical path. Guarded
reload and refresh resolve that id through the current typed-source map under the lifecycle lock,
then validate the expected account id; they do not rerun automatic startup selection. Pin/repin
accepts the initiating manager's active lease, blocks if another process owns either affected
account lease, and remaps the initiating manager after commit. This keeps active sessions valid
when their account moves between profile and root, and keeps an explicitly selected
automation-disabled alternate refreshable while a different account is pinned at root.

- Explicit selection may activate any enabled, valid source, including the main.
- While `automatic_selection_suspended` is set, startup/reload returns no managed auth and generic
  root fallback remains blocked; only explicit selection/login clears the suspension.
- Automatic startup/failover considers only `automation_enabled` sources and the existing usage,
  lease, login-required, and workspace filters.
- If the main is automation-disabled and no other automatic candidate exists, automatic startup
  returns no auth; it must not use raw root.
- Refresh of an active main persists to root under root/index locks and updates the root fingerprint
  metadata. Alternate refresh continues to persist to its profile.
- Same-identity generation drift seen outside a fork transaction is metadata-only adoption after
  validation; identity drift or drift during mutation blocks.

## Staged delivery

Keep each complex PR below 500 changed lines excluding mechanical snapshots.

1. **Atomic file storage and lifecycle primitives (about 350 lines).** Land the generic atomic
   `FileAuthStorage` recovery fix as its own commit. Add the fork-owned lifecycle state file, file-only mode
   validation, rename journal/recovery, root fingerprinting, pin/repin/unpin, and fault-injection
   tests at every journal/write/rename boundary. No UI.
2. **Managed source selection and login staging (about 450 lines).** Reuse the landed granular
   `/accounts` automation metadata, route pinned root through typed account sources, eliminate raw-root fallback, preserve
   `automation_enabled`, stage CLI account add and direct managed login, and define targeted
   logout/re-import/replacement. Add login and core integration tests.
3. **App-server and TUI integration (about 350 lines plus snapshots).** Reuse the same staging and
   lifecycle service from app-server writers without wire changes; wire `/accounts` set-main and
   status/reconcile display; document pinned-main login rejection, process-local token exemption,
   and targeted-logout semantics with examples in `app-server/README.md`; add public JSON-RPC tests
   and TUI snapshots. Do not duplicate lifecycle logic in either UI crate.

Slice 1 must land before the other two. Slice 2 can then precede or be developed alongside the UI
portion of slice 3, but app-server login integration depends on the shared staging API from slice 2.

## Canonical validation

- `codex-login` unit tests: full-object index/state assertions; fault injection after journal save,
  every rename, final lifecycle/index saves, legacy index rewrites while a journal exists,
  recoverable raw-auth pending files through save/load/delete at every boundary,
  deletion-intent and tombstone recovery at every POSIX and Windows boundary including save after
  interrupted delete,
  every `AuthDotJson` mode fingerprint, same-identity replacement with equal and rotated refresh
  tokens never revoking the retained grant, replacement A-to-B while profile A remains never
  revoking A, canceled/orphaned same-account staging cleanup, drift,
  corrupt/missing files, marker
  migration, alternate-profile replacement, full-root and legacy-marker import-current, pinned
  import-current, crash-recovered import-current preserving caller label and affected profile
  post-images, live staging-lease
  protection, malicious journal ids, pre-move file and post-move source/destination directory
  syncing, Unix `0o600` replacement, post-pin store-mode changes, no-clobber marker installation,
  every targeted-logout boundary with remaining
  profiles, logout-then-add into missing profile auth, missing/corrupt-main reconciliation with
  bounded raw-byte fingerprint plus oversized rejection, and
  cleanup-pending restart/retry through confirmed revocation and durable tombstone removal,
  journal-conflict finish/abort, journal lock-set races, active-session pin/repin remapping,
  occupied-root initial pin, Windows write-through-handle rename/replacement and logical-delete
  resurrection, Windows non-NTFS lifecycle rejection while ordinary index mutation still succeeds,
  cross-device layouts,
  symlink/junction path escapes, and rejection of pinned
  keyring/auto/ephemeral.
- `codex-core` integration tests: pinned root is a managed source; explicit main selection works;
  `automation_enabled = false` cannot fall through raw root; refresh/failover update the correct
  authority; an explicitly selected automation-disabled alternate reloads/refreshes from its typed
  profile source; forced workspace mismatch is non-destructive.
- CLI login tests: canceled/failed file-mode login preserves root; non-file unpinned modes retain
  upstream behavior; account add never uses root as staging; same-id main reauth replaces
  atomically; no-main direct login matching an imported profile retires the profile credential and
  leaves exactly one root authority; different-id direct login is rejected; after a crash with
  pending root refresh, `reconcile-main` promotes it before a simulated unmodified file loader reads
  root.
- App-server v2 tests through public JSON-RPC: browser/device success and failure, imported-profile
  identity collision, same-id replacement, guarded API-key login, process-local
  `ChatgptAuthTokens` while main is pinned with no
  root mutation, external-auth logout clearing only process-local state, targeted managed logout,
  unchanged protocol fixtures, and matching README examples.
- TUI snapshots: `/accounts` identifies main, preserves its automation toggle, and exposes pending
  reconcile finish/restore/cancel choices. Pinning itself does not alter the toggle.
- Cross-process test: mutate root to a different identity between validated boundaries and assert
  the fork reports drift without overwriting root, profile, staging, or index.
- Per slice: scoped `just test`, scoped `just fix`, `just fmt`, pending snapshot review where
  applicable, `git diff --check`, and the assigned Review Suite profile.

## Explicit non-goals

- No guarantee for simultaneous mutation by an unmodified desktop/non-fork client.
- No guarantee for launching an unmodified client first after abnormal fork termination; run the
  fork recovery gate before that reverse handoff.
- No defense against an adversarial same-user process racing Windows namespace components; that
  process can already read or replace the user's auth directly. Persisted malformed/reparse state
  is still rejected.
- No keyring/auto pinning until a shared transactional credential broker exists.
- No persistent per-account `CODEX_HOME`, duplicate profile copy of main auth, app-server wire
  change, desktop patch, new dependency, or automatic adoption of a different root identity.
- No rename-only churn of existing standalone fork files.
