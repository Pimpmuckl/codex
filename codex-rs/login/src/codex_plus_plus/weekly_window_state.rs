use super::AccountId;
use super::AccountStore;
use super::replace_file;
use crate::account_lease::AccountLease;
use serde::Deserialize;
use serde::Serialize;
use std::fs::File;
use std::io::Read;
use std::io::Write;
use std::io::{self};
use std::path::Path;
use std::path::PathBuf;
const STATE_FILE: &str = "weekly-window-state.json";
const LOCK_FILE: &str = "weekly-window.lock";
const SCAN_LOCK_FILE: &str = "weekly-window-scan.lock";
const MAX_STATE_BYTES: u64 = 4 * 1024;
const SUPPRESSION_SECONDS: i64 = 7 * 24 * 60 * 60;
const MAX_FAILURE_COUNT: u8 = 8;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WeeklyWindowUsage {
    Missing,
    Present {
        unused: bool,
        resets_at: Option<i64>,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WeeklyWindowError {
    Ambiguous,
    Transient,
    LoginRequired,
    Rejected,
    StateQuarantined,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WeeklyWindowRetryableError {
    Transient,
    LoginRequired,
    Rejected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WeeklyWindowAttemptOutcome {
    Completed { refreshed_usage: WeeklyWindowUsage },
    Retryable { error: WeeklyWindowRetryableError },
    Ambiguous,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WeeklyWindowStatus {
    pub last_error: Option<WeeklyWindowError>,
    pub retry_not_before: Option<i64>,
    pub recovery_not_before: Option<i64>,
}

pub enum WeeklyWindowAttemptDecision {
    NotDue,
    Locked,
    StateUnavailable,
    Ready(WeeklyWindowAttempt),
}

#[must_use = "finish the attempt so its durable state records the outcome"]
pub struct WeeklyWindowAttempt {
    state_path: PathBuf,
    state: State,
    _lease: AccountLease,
}

pub struct WeeklyWindowScanLease {
    _lease: AccountLease,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
enum AttemptIdentity {
    ResetAt(i64),
    MissingReset(u32),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum AttemptStatus {
    Dispatching,
    Retryable,
    Closed,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct State {
    version: u8,
    last_observed_reset_at: Option<i64>,
    last_observed_active: bool,
    missing_reset_generation: u32,
    attempt_identity: Option<AttemptIdentity>,
    attempt_status: Option<AttemptStatus>,
    last_attempt_at: Option<i64>,
    failure_count: u8,
    retry_not_before: Option<i64>,
    recovery_not_before: Option<i64>,
    last_error: Option<WeeklyWindowError>,
}

impl State {
    fn new() -> Self {
        Self {
            version: 1,
            ..Self::default()
        }
    }
}

impl AccountStore {
    pub fn try_acquire_weekly_window_scan(&self) -> io::Result<Option<WeeklyWindowScanLease>> {
        AccountLease::try_acquire(&self.codex_home.join(SCAN_LOCK_FILE))
            .map(|lease| lease.map(|_lease| WeeklyWindowScanLease { _lease }))
    }

    pub fn begin_weekly_window_attempt(
        &self,
        account_id: &AccountId,
        usage: WeeklyWindowUsage,
        now: i64,
    ) -> io::Result<WeeklyWindowAttemptDecision> {
        let account_home = self.account_home(account_id);
        let Some(lease) = AccountLease::try_acquire(&account_home.join(LOCK_FILE))? else {
            return Ok(WeeklyWindowAttemptDecision::Locked);
        };
        let eligible = self.list()?.into_iter().any(|profile| {
            profile.id == *account_id
                && profile.enabled
                && !profile.login_required
                && profile.automation_enabled
        });
        if !eligible {
            return Ok(WeeklyWindowAttemptDecision::NotDue);
        }

        let state_path = account_home.join(STATE_FILE);
        let mut state = match read_state(&state_path)? {
            StateRead::Ready(state) => state,
            StateRead::Corrupt => {
                let state = quarantine_state(usage, now);
                write_state(&state_path, &state)?;
                return Ok(WeeklyWindowAttemptDecision::StateUnavailable);
            }
            StateRead::Incompatible => return Ok(WeeklyWindowAttemptDecision::StateUnavailable),
        };

        if state.attempt_status == Some(AttemptStatus::Dispatching) {
            let dispatched_at = state.last_attempt_at;
            close_attempt(&mut state, now, Some(WeeklyWindowError::Ambiguous));
            state.last_attempt_at = dispatched_at;
            write_state(&state_path, &state)?;
            return Ok(WeeklyWindowAttemptDecision::NotDue);
        }

        let Some(identity) = due_identity(&mut state, usage, now) else {
            write_state(&state_path, &state)?;
            return Ok(WeeklyWindowAttemptDecision::NotDue);
        };
        if state.attempt_identity != Some(identity) {
            state.failure_count = 0;
            state.retry_not_before = None;
            state.last_error = None;
        }
        state.attempt_identity = Some(identity);
        state.attempt_status = Some(AttemptStatus::Dispatching);
        state.last_attempt_at = Some(now);
        write_state(&state_path, &state)?;
        Ok(WeeklyWindowAttemptDecision::Ready(WeeklyWindowAttempt {
            state_path,
            state,
            _lease: lease,
        }))
    }

    pub fn weekly_window_status(&self, account_id: &AccountId) -> io::Result<WeeklyWindowStatus> {
        let status = match read_state(&self.account_home(account_id).join(STATE_FILE))? {
            StateRead::Ready(state) => WeeklyWindowStatus {
                last_error: state.last_error,
                retry_not_before: state.retry_not_before,
                recovery_not_before: state.recovery_not_before,
            },
            StateRead::Corrupt | StateRead::Incompatible => WeeklyWindowStatus {
                last_error: Some(WeeklyWindowError::StateQuarantined),
                ..WeeklyWindowStatus::default()
            },
        };
        Ok(status)
    }
}

impl WeeklyWindowAttempt {
    pub fn finish(mut self, outcome: WeeklyWindowAttemptOutcome, now: i64) -> io::Result<()> {
        match outcome {
            WeeklyWindowAttemptOutcome::Completed { refreshed_usage } => {
                close_attempt(&mut self.state, now, /*error*/ None);
                observe_after_completion(&mut self.state, refreshed_usage);
            }
            WeeklyWindowAttemptOutcome::Retryable { error } => {
                let delay = (5 * 60_i64)
                    .saturating_mul(1_i64 << self.state.failure_count.min(MAX_FAILURE_COUNT))
                    .min(6 * 60 * 60);
                self.state.failure_count = self
                    .state
                    .failure_count
                    .saturating_add(1)
                    .min(MAX_FAILURE_COUNT);
                self.state.attempt_status = Some(AttemptStatus::Retryable);
                self.state.last_attempt_at = Some(now);
                self.state.retry_not_before = Some(now.saturating_add(delay));
                self.state.last_error = Some(match error {
                    WeeklyWindowRetryableError::Transient => WeeklyWindowError::Transient,
                    WeeklyWindowRetryableError::LoginRequired => WeeklyWindowError::LoginRequired,
                    WeeklyWindowRetryableError::Rejected => WeeklyWindowError::Rejected,
                });
            }
            WeeklyWindowAttemptOutcome::Ambiguous => {
                close_attempt(&mut self.state, now, Some(WeeklyWindowError::Ambiguous));
            }
        }
        write_state(&self.state_path, &self.state)
    }
}

fn due_identity(state: &mut State, usage: WeeklyWindowUsage, now: i64) -> Option<AttemptIdentity> {
    let WeeklyWindowUsage::Present { unused, resets_at } = usage else {
        return None;
    };
    if !unused {
        if state.attempt_status == Some(AttemptStatus::Retryable) {
            close_attempt(state, now, /*error*/ None);
        }
        state.last_observed_active = true;
        state.last_observed_reset_at = resets_at.or(state.last_observed_reset_at);
        return None;
    }

    if let Some(until) = state.recovery_not_before {
        if now < until && !state.last_observed_active {
            baseline(state, resets_at);
            return None;
        }
        state.recovery_not_before = None;
        state.last_error = None;
    }

    let identity = if let Some(resets_at) = resets_at {
        let identity = AttemptIdentity::ResetAt(resets_at);
        if state.attempt_status == Some(AttemptStatus::Retryable)
            && state.attempt_identity == Some(identity)
        {
            if state
                .retry_not_before
                .is_some_and(|retry_at| now < retry_at)
            {
                return None;
            }
            identity
        } else {
            if state.attempt_status == Some(AttemptStatus::Closed)
                && state.attempt_identity == Some(identity)
            {
                return None;
            }
            let moved_forward = state
                .last_observed_reset_at
                .is_some_and(|previous| resets_at > previous);
            if resets_at > now && !moved_forward {
                baseline(state, Some(resets_at));
                return None;
            }
            identity
        }
    } else if state.attempt_status == Some(AttemptStatus::Retryable) {
        if state
            .retry_not_before
            .is_some_and(|retry_at| now < retry_at)
        {
            return None;
        }
        state.attempt_identity?
    } else {
        if state.attempt_status == Some(AttemptStatus::Closed)
            && matches!(state.attempt_identity, Some(AttemptIdentity::ResetAt(_)))
            && inside_suppression(state, now)
            && !state.last_observed_active
        {
            state.last_observed_active = false;
            return None;
        }
        let generation = if state.last_observed_active
            || state.attempt_status == Some(AttemptStatus::Closed)
                && !inside_suppression(state, now)
        {
            state.missing_reset_generation.saturating_add(1).max(1)
        } else if state.missing_reset_generation == 0 {
            1
        } else if state.attempt_status.is_none() {
            state.missing_reset_generation
        } else {
            return None;
        };
        state.missing_reset_generation = generation;
        AttemptIdentity::MissingReset(generation)
    };

    if state.attempt_status == Some(AttemptStatus::Closed)
        && state.attempt_identity != Some(identity)
        && !state.last_observed_active
        && inside_suppression(state, now)
    {
        baseline(state, resets_at);
        return None;
    }
    state.last_observed_active = false;
    Some(identity)
}

fn baseline(state: &mut State, resets_at: Option<i64>) {
    state.last_observed_reset_at = resets_at.or(state.last_observed_reset_at);
    state.last_observed_active = false;
    if resets_at.is_none() && state.missing_reset_generation == 0 {
        state.missing_reset_generation = 1;
    }
}

fn close_attempt(state: &mut State, now: i64, error: Option<WeeklyWindowError>) {
    if let Some(AttemptIdentity::ResetAt(resets_at)) = state.attempt_identity {
        state.last_observed_reset_at = Some(resets_at);
    }
    state.attempt_status = Some(AttemptStatus::Closed);
    state.last_attempt_at = Some(now);
    state.failure_count = 0;
    state.retry_not_before = None;
    state.recovery_not_before = None;
    state.last_error = error;
    state.last_observed_active = false;
}

fn observe_after_completion(state: &mut State, usage: WeeklyWindowUsage) {
    if let WeeklyWindowUsage::Present { unused, resets_at } = usage {
        state.last_observed_active = !unused;
        state.last_observed_reset_at = resets_at.or(state.last_observed_reset_at);
    }
}

fn inside_suppression(state: &State, now: i64) -> bool {
    state
        .last_attempt_at
        .is_some_and(|at| now < at.saturating_add(SUPPRESSION_SECONDS))
}

fn quarantine_state(usage: WeeklyWindowUsage, now: i64) -> State {
    let mut state = State {
        recovery_not_before: Some(now.saturating_add(SUPPRESSION_SECONDS)),
        last_error: Some(WeeklyWindowError::StateQuarantined),
        ..State::new()
    };
    if let WeeklyWindowUsage::Present { unused, resets_at } = usage {
        state.last_observed_active = !unused;
        state.last_observed_reset_at = resets_at;
        if unused {
            baseline(&mut state, resets_at);
        }
    }
    state
}

enum StateRead {
    Ready(State),
    Corrupt,
    Incompatible,
}

fn read_state(path: &Path) -> io::Result<StateRead> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Ok(StateRead::Ready(State::new()));
        }
        Err(err) => return Err(err),
    };
    let mut bytes = Vec::new();
    Read::take(file, MAX_STATE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        return Ok(StateRead::Incompatible);
    }
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return Ok(StateRead::Corrupt);
    };
    let Some(version) = value.get("version").and_then(serde_json::Value::as_u64) else {
        return Ok(StateRead::Corrupt);
    };
    if version != 1 {
        return Ok(StateRead::Incompatible);
    }
    Ok(serde_json::from_value(value).map_or(StateRead::Corrupt, StateRead::Ready))
}

fn write_state(path: &Path, state: &State) -> io::Result<()> {
    let bytes = serde_json::to_vec(state).map_err(io::Error::other)?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        return Err(io::Error::other("weekly window state too large"));
    }
    let parent = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
    std::fs::create_dir_all(parent)?;
    let temp_path = parent.join("weekly-window-state.json.tmp");
    let mut file = File::create(&temp_path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    replace_file(&temp_path, path)?;
    #[cfg(not(windows))]
    File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
#[path = "weekly_window_state_tests.rs"]
mod tests;
