use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const STORE_VERSION: u32 = 1;
const STORE_FILE: &str = "checkpoint-episodes.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpisodeKey {
    pub agent: String,
    pub project_id: String,
    pub session_id: String,
    pub reset_at_ms: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EpisodeState {
    Detected,
    // Older releases reissued AskIssued after a 30-second lease.
    AskIssued,
    // The host owns the question and may deliver the reply in a later turn.
    AwaitingDecision,
    AwaitingCustomInput,
    CapsulePending,
    CapsuleCommitted,
    ResumeIssued,
    Completed,
    Skipped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserDecision {
    Save,
    Skip,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Episode {
    pub episode_id: String,
    pub key: EpisodeKey,
    pub state: EpisodeState,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    #[serde(default)]
    pub reset_estimated: bool,
    #[serde(default)]
    pub decision: Option<UserDecision>,
    #[serde(default)]
    pub custom_instruction: Option<String>,
    #[serde(default)]
    pub capsule_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResumeInstruction {
    pub episode_id: String,
    pub skipped: bool,
    pub custom_instruction: Option<String>,
    pub capsule_id: Option<String>,
}

#[derive(Clone)]
pub struct EpisodeStore {
    home: PathBuf,
    gate: Arc<Mutex<()>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct EpisodeFile {
    version: u32,
    #[serde(default)]
    episodes: BTreeMap<String, Episode>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    question_guides: BTreeMap<String, i64>,
}

impl Default for EpisodeFile {
    fn default() -> Self {
        Self {
            version: STORE_VERSION,
            episodes: BTreeMap::new(),
            question_guides: BTreeMap::new(),
        }
    }
}

impl EpisodeStore {
    pub fn new(home: impl AsRef<Path>) -> Self {
        Self {
            home: home.as_ref().to_path_buf(),
            gate: Arc::new(Mutex::new(())),
        }
    }

    pub fn path(&self) -> PathBuf {
        self.home.join("store").join(STORE_FILE)
    }

    /// Preserve the guide across a daemon restart between prompt and Stop.
    pub(crate) fn prepare_question_guide(
        &self,
        key: &str,
        now_ms: i64,
        refresh: bool,
    ) -> anyhow::Result<bool> {
        let _guard = self
            .gate
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let path = self.path();
        let mut data = read_file(&path)?;
        if !refresh
            && data
                .question_guides
                .get(key)
                .is_some_and(|until| *until > now_ms)
        {
            return Ok(false);
        }
        data.question_guides.retain(|_, until| *until > now_ms);
        data.question_guides.insert(
            key.to_owned(),
            now_ms.saturating_add(crate::trigger_mark::FIVE_HOUR_WINDOW_MS),
        );
        write_file(&path, &data)?;
        Ok(true)
    }

    pub(crate) fn has_question_guide(&self, key: &str, now_ms: i64) -> anyhow::Result<bool> {
        let _guard = self
            .gate
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        Ok(read_file(&self.path())?
            .question_guides
            .get(key)
            .is_some_and(|until| *until > now_ms))
    }

    pub fn begin_or_load(&self, key: EpisodeKey, now_ms: i64) -> anyhow::Result<Episode> {
        self.mutate(now_ms, |data| {
            let episode_id = episode_id(&key);
            let episode = data
                .episodes
                .entry(episode_id.clone())
                .or_insert_with(|| Episode {
                    episode_id,
                    key,
                    state: EpisodeState::Detected,
                    created_at_ms: now_ms,
                    updated_at_ms: now_ms,
                    reset_estimated: false,
                    decision: None,
                    custom_instruction: None,
                    capsule_id: None,
                });
            Ok(episode.clone())
        })
    }

    /// Persist the offer before emitting hook output. An unanswered, cancelled
    /// or unobserved host question counts for the entire window, including
    /// after the daemon restarts. The singleton daemon serializes these writes.
    pub fn offer_question(
        &self,
        agent: &str,
        project_id: &str,
        session_id: &str,
        reported_reset: Option<i64>,
        now_ms: i64,
    ) -> anyhow::Result<(Episode, bool)> {
        let reset = reported_reset.filter(|time| *time > now_ms);
        let _guard = self.gate.lock().unwrap_or_else(|p| p.into_inner());
        let path = self.path();
        let mut data = read_file(&path)?;
        data.episodes.retain(|_, episode| {
            // A reported reset more than five hours after our first offer
            // belongs to a later window. Allow one second for clock rounding.
            let next_window = episode.reset_estimated
                && episode.key.agent == agent
                && episode.key.project_id == project_id
                && episode.key.session_id == session_id
                && reset.is_some_and(|reset| reset > episode.key.reset_at_ms.saturating_add(1000));
            episode.key.reset_at_ms > now_ms && !next_window
        });
        let active = data
            .episodes
            .values_mut()
            .filter(|e| {
                e.key.agent == agent
                    && e.key.project_id == project_id
                    && e.key.session_id == session_id
            })
            .max_by_key(|e| e.created_at_ms);
        let (episode, issued) = if let Some(episode) = active {
            let issue = episode.state == EpisodeState::Detected;
            let correct_reset = reset.filter(|_| episode.reset_estimated);
            if !issue && correct_reset.is_none() {
                return Ok((episode.clone(), false));
            }
            if let Some(reset) = correct_reset {
                episode.key.reset_at_ms = reset;
                episode.reset_estimated = false;
            }
            if issue {
                episode.state = EpisodeState::AwaitingDecision;
            }
            episode.updated_at_ms = now_ms;
            (episode.clone(), issue)
        } else {
            let key = EpisodeKey {
                agent: agent.into(),
                project_id: project_id.into(),
                session_id: session_id.into(),
                reset_at_ms: reset.unwrap_or_else(|| {
                    now_ms.saturating_add(crate::trigger_mark::FIVE_HOUR_WINDOW_MS)
                }),
            };
            let episode = Episode {
                episode_id: episode_id(&key),
                key,
                state: EpisodeState::AwaitingDecision,
                created_at_ms: now_ms,
                updated_at_ms: now_ms,
                reset_estimated: reset.is_none(),
                decision: None,
                custom_instruction: None,
                capsule_id: None,
            };
            data.episodes
                .insert(episode.episode_id.clone(), episode.clone());
            (episode, true)
        };
        write_file(&path, &data)?;
        Ok((episode, issued))
    }

    pub fn get(&self, episode_id: &str) -> anyhow::Result<Option<Episode>> {
        let _guard = self
            .gate
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        Ok(read_file(&self.path())?.episodes.get(episode_id).cloned())
    }

    pub fn find_active(
        &self,
        agent: &str,
        project_id: &str,
        session_id: &str,
        now_ms: i64,
    ) -> anyhow::Result<Option<Episode>> {
        let _guard = self
            .gate
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let data = read_file(&self.path())?;
        Ok(data
            .episodes
            .values()
            .filter(|episode| {
                episode.key.agent == agent
                    && episode.key.project_id == project_id
                    && episode.key.session_id == session_id
                    && episode.key.reset_at_ms > now_ms
            })
            .max_by_key(|episode| episode.created_at_ms)
            .cloned())
    }

    pub fn record_decision(
        &self,
        episode_id: &str,
        decision: UserDecision,
        now_ms: i64,
    ) -> anyhow::Result<Episode> {
        self.mutate(now_ms, |data| {
            let episode = required_episode(data, episode_id)?;
            if let Some(existing) = episode.decision {
                anyhow::ensure!(
                    existing == decision,
                    "episode already has a different decision"
                );
                return Ok(episode.clone());
            }
            anyhow::ensure!(
                matches!(
                    episode.state,
                    EpisodeState::Detected
                        | EpisodeState::AskIssued
                        | EpisodeState::AwaitingDecision
                ),
                "episode is not awaiting a decision"
            );
            episode.decision = Some(decision);
            episode.state = match decision {
                UserDecision::Save => EpisodeState::CapsulePending,
                UserDecision::Skip => EpisodeState::Skipped,
                UserDecision::Other => EpisodeState::AwaitingCustomInput,
            };
            episode.updated_at_ms = now_ms;
            Ok(episode.clone())
        })
    }

    pub fn record_custom_instruction(
        &self,
        episode_id: &str,
        instruction: &str,
        now_ms: i64,
    ) -> anyhow::Result<Episode> {
        let instruction = instruction.trim();
        anyhow::ensure!(
            !instruction.is_empty(),
            "custom instruction cannot be empty"
        );
        self.mutate(now_ms, |data| {
            let episode = required_episode(data, episode_id)?;
            anyhow::ensure!(
                episode.state == EpisodeState::AwaitingCustomInput,
                "episode is not awaiting custom input"
            );
            episode.custom_instruction = Some(instruction.to_string());
            episode.state = EpisodeState::CapsulePending;
            episode.updated_at_ms = now_ms;
            Ok(episode.clone())
        })
    }

    pub fn commit_capsule(
        &self,
        episode_id: &str,
        capsule_id: &str,
        now_ms: i64,
    ) -> anyhow::Result<Episode> {
        anyhow::ensure!(!capsule_id.trim().is_empty(), "capsule id cannot be empty");
        self.mutate(now_ms, |data| {
            let episode = required_episode(data, episode_id)?;
            if let Some(existing) = &episode.capsule_id {
                anyhow::ensure!(
                    existing == capsule_id,
                    "episode already committed another capsule"
                );
                return Ok(episode.clone());
            }
            anyhow::ensure!(
                episode.state == EpisodeState::CapsulePending,
                "episode is not waiting for a capsule commit"
            );
            episode.capsule_id = Some(capsule_id.to_string());
            episode.state = EpisodeState::CapsuleCommitted;
            episode.updated_at_ms = now_ms;
            Ok(episode.clone())
        })
    }

    pub fn take_resume(
        &self,
        episode_id: &str,
        now_ms: i64,
    ) -> anyhow::Result<Option<ResumeInstruction>> {
        self.mutate(now_ms, |data| {
            let episode = required_episode(data, episode_id)?;
            if !matches!(
                episode.state,
                EpisodeState::CapsuleCommitted | EpisodeState::Skipped
            ) {
                return Ok(None);
            }
            let resume = ResumeInstruction {
                episode_id: episode.episode_id.clone(),
                skipped: episode.decision == Some(UserDecision::Skip),
                custom_instruction: episode.custom_instruction.clone(),
                capsule_id: episode.capsule_id.clone(),
            };
            episode.state = EpisodeState::ResumeIssued;
            episode.updated_at_ms = now_ms;
            Ok(Some(resume))
        })
    }

    pub fn complete_resume(&self, episode_id: &str, now_ms: i64) -> anyhow::Result<Episode> {
        self.mutate(now_ms, |data| {
            let episode = required_episode(data, episode_id)?;
            if episode.state == EpisodeState::Completed {
                return Ok(episode.clone());
            }
            anyhow::ensure!(
                episode.state == EpisodeState::ResumeIssued,
                "episode has not issued a resume instruction"
            );
            episode.state = EpisodeState::Completed;
            episode.updated_at_ms = now_ms;
            Ok(episode.clone())
        })
    }

    fn mutate<T>(
        &self,
        now_ms: i64,
        mutate: impl FnOnce(&mut EpisodeFile) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        let _guard = self
            .gate
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let path = self.path();
        let mut data = read_file(&path)?;
        data.episodes
            .retain(|_, episode| episode.key.reset_at_ms > now_ms);
        let result = mutate(&mut data)?;
        write_file(&path, &data)?;
        Ok(result)
    }
}

fn write_file(path: &Path, data: &EpisodeFile) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec_pretty(data)?;
    Ok(write_atomic_with(path, &bytes, |from, to| {
        std::fs::rename(from, to)
    })?)
}

fn required_episode<'a>(
    data: &'a mut EpisodeFile,
    episode_id: &str,
) -> anyhow::Result<&'a mut Episode> {
    data.episodes
        .get_mut(episode_id)
        .ok_or_else(|| anyhow::anyhow!("checkpoint episode not found: {episode_id}"))
}

fn episode_id(key: &EpisodeKey) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.agent.as_bytes());
    hasher.update([0]);
    hasher.update(key.project_id.as_bytes());
    hasher.update([0]);
    hasher.update(key.session_id.as_bytes());
    hasher.update([0]);
    hasher.update(key.reset_at_ms.to_le_bytes());
    let digest = hasher.finalize();
    let mut id = String::from("episode-");
    for byte in &digest[..12] {
        use std::fmt::Write as _;
        let _ = write!(&mut id, "{byte:02x}");
    }
    id
}

fn read_file(path: &Path) -> anyhow::Result<EpisodeFile> {
    let backup = path.with_extension("json.bak");
    if !path.exists() && backup.exists() {
        std::fs::rename(&backup, path)?;
    }
    match std::fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(EpisodeFile::default()),
        Err(error) => Err(error.into()),
    }
}

fn write_atomic_with<R>(path: &Path, bytes: &[u8], mut rename: R) -> std::io::Result<()>
where
    R: FnMut(&Path, &Path) -> std::io::Result<()>,
{
    if let Some(parent) = path.parent() {
        ai_handoff_core::secure_fs::ensure_private_dir(parent)?;
    }
    let temporary = path.with_extension("json.tmp");
    let backup = path.with_extension("json.bak");
    let _ = std::fs::remove_file(&temporary);
    if path.exists() {
        let _ = std::fs::remove_file(&backup);
    }
    ai_handoff_core::secure_fs::write_private_file(&temporary, bytes)?;

    if !path.exists() {
        return match rename(&temporary, path) {
            Ok(()) => Ok(()),
            Err(error) => {
                let _ = std::fs::remove_file(&temporary);
                Err(error)
            }
        };
    }

    if let Err(error) = rename(path, &backup) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    match rename(&temporary, path) {
        Ok(()) => {
            let _ = std::fs::remove_file(&backup);
            Ok(())
        }
        Err(error) => {
            let _ = std::fs::remove_file(&temporary);
            if let Err(restore_error) = rename(&backup, path) {
                return Err(std::io::Error::other(format!(
                    "{error}; restoring prior checkpoint episodes failed: {restore_error}"
                )));
            }
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(project: &str, session: &str, reset_at_ms: i64) -> EpisodeKey {
        EpisodeKey {
            agent: "codex".into(),
            project_id: project.into(),
            session_id: session.into(),
            reset_at_ms,
        }
    }

    #[test]
    fn separate_projects_and_sessions_get_separate_episodes() {
        let home = tempfile::tempdir().unwrap();
        let store = EpisodeStore::new(home.path());

        let first = store.begin_or_load(key("p1", "s1", 10_000), 100).unwrap();
        let other_session = store.begin_or_load(key("p1", "s2", 10_000), 100).unwrap();
        let other_project = store.begin_or_load(key("p2", "s1", 10_000), 100).unwrap();

        assert_ne!(first.episode_id, other_session.episode_id);
        assert_ne!(first.episode_id, other_project.episode_id);
        assert_eq!(store.get(&first.episode_id).unwrap().unwrap(), first);
    }

    #[test]
    fn question_survives_expired_lease_and_restart_until_window_reset() {
        let home = tempfile::tempdir().unwrap();
        let store = EpisodeStore::new(home.path());
        let offer = |store: &EpisodeStore, now, reset| {
            store
                .offer_question("claude-code", "p", "s", reset, now)
                .unwrap()
        };
        let (first, fired) = offer(&store, 100, None);
        assert!(fired);
        assert_eq!(
            first.key.reset_at_ms,
            100 + crate::trigger_mark::FIVE_HOUR_WINDOW_MS
        );
        let reopened = EpisodeStore::new(home.path());
        let (same, fired) = offer(&reopened, 35_000, Some(50_000));
        assert!(!fired);
        assert_eq!(first.episode_id, same.episode_id);
        assert_eq!(same.key.reset_at_ms, 50_000);
        assert!(!offer(&reopened, 49_999, None).1);
        let (next, fired) = offer(&reopened, 50_000, Some(75_000));
        assert!(fired);
        assert_ne!(first.episode_id, next.episode_id);
    }

    #[test]
    fn confirmed_new_window_replaces_an_estimated_expiry() {
        let home = tempfile::tempdir().unwrap();
        let store = EpisodeStore::new(home.path());
        let (old, _) = store
            .offer_question("codex", "p", "s", None, 1_000)
            .unwrap();
        let next_reset = old.key.reset_at_ms + 60_000;
        let (new, issued) = store
            .offer_question("codex", "p", "s", Some(next_reset), 70_000)
            .unwrap();
        assert!(issued);
        assert_ne!(old.episode_id, new.episode_id);
        assert!(
            !store
                .offer_question("codex", "p", "s", Some(next_reset), 75_000)
                .unwrap()
                .1
        );
    }

    #[test]
    fn save_decision_commits_once_and_resumes_once() {
        let home = tempfile::tempdir().unwrap();
        let store = EpisodeStore::new(home.path());
        let episode = store.begin_or_load(key("p", "s", 10_000), 100).unwrap();

        let pending = store
            .record_decision(&episode.episode_id, UserDecision::Save, 110)
            .unwrap();
        assert_eq!(pending.state, EpisodeState::CapsulePending);
        let committed = store
            .commit_capsule(&episode.episode_id, "cap-1", 120)
            .unwrap();
        assert_eq!(committed.state, EpisodeState::CapsuleCommitted);
        assert_eq!(committed.capsule_id.as_deref(), Some("cap-1"));
        assert_eq!(
            store
                .commit_capsule(&episode.episode_id, "cap-1", 121)
                .unwrap(),
            committed
        );

        let resume = store
            .take_resume(&episode.episode_id, 130)
            .unwrap()
            .unwrap();
        assert!(!resume.skipped);
        assert_eq!(resume.capsule_id.as_deref(), Some("cap-1"));
        assert!(store
            .take_resume(&episode.episode_id, 131)
            .unwrap()
            .is_none());
        let completed = store.complete_resume(&episode.episode_id, 140).unwrap();
        assert_eq!(completed.state, EpisodeState::Completed);
    }

    #[test]
    fn skip_decision_resumes_without_a_capsule() {
        let home = tempfile::tempdir().unwrap();
        let store = EpisodeStore::new(home.path());
        let episode = store.begin_or_load(key("p", "s", 10_000), 100).unwrap();

        let skipped = store
            .record_decision(&episode.episode_id, UserDecision::Skip, 110)
            .unwrap();
        assert_eq!(skipped.state, EpisodeState::Skipped);
        let resume = store
            .take_resume(&episode.episode_id, 120)
            .unwrap()
            .unwrap();
        assert!(resume.skipped);
        assert!(resume.capsule_id.is_none());
    }

    #[test]
    fn other_decision_waits_for_custom_instruction_then_saves() {
        let home = tempfile::tempdir().unwrap();
        let store = EpisodeStore::new(home.path());
        let episode = store.begin_or_load(key("p", "s", 10_000), 100).unwrap();

        let awaiting = store
            .record_decision(&episode.episode_id, UserDecision::Other, 110)
            .unwrap();
        assert_eq!(awaiting.state, EpisodeState::AwaitingCustomInput);
        let pending = store
            .record_custom_instruction(&episode.episode_id, "include test logs", 120)
            .unwrap();
        assert_eq!(pending.state, EpisodeState::CapsulePending);
        assert_eq!(
            pending.custom_instruction.as_deref(),
            Some("include test logs")
        );
    }

    #[test]
    fn episodes_reload_from_json() {
        let home = tempfile::tempdir().unwrap();
        let first_store = EpisodeStore::new(home.path());
        let episode = first_store
            .begin_or_load(key("p", "s", 10_000), 100)
            .unwrap();
        first_store
            .record_decision(&episode.episode_id, UserDecision::Save, 110)
            .unwrap();

        let reloaded = EpisodeStore::new(home.path())
            .get(&episode.episode_id)
            .unwrap()
            .unwrap();
        assert_eq!(reloaded.state, EpisodeState::CapsulePending);
    }

    #[test]
    fn finds_the_active_episode_without_knowing_reset_time() {
        let home = tempfile::tempdir().unwrap();
        let store = EpisodeStore::new(home.path());
        let episode = store.begin_or_load(key("p", "s", 10_000), 100).unwrap();

        assert_eq!(
            store.find_active("codex", "p", "s", 500).unwrap(),
            Some(episode)
        );
        assert!(store
            .find_active("codex", "p", "other", 500)
            .unwrap()
            .is_none());
        assert!(store
            .find_active("codex", "p", "s", 10_000)
            .unwrap()
            .is_none());
    }

    #[test]
    fn failed_atomic_replace_restores_the_prior_file() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("checkpoint-episodes.json");
        std::fs::write(&path, b"prior").unwrap();
        let calls = std::cell::Cell::new(0);

        let error = write_atomic_with(&path, b"next", |from, to| {
            calls.set(calls.get() + 1);
            if calls.get() == 2 {
                Err(std::io::Error::other("simulated replace failure"))
            } else {
                std::fs::rename(from, to)
            }
        })
        .unwrap_err();

        assert!(error.to_string().contains("simulated replace failure"));
        assert_eq!(std::fs::read(&path).unwrap(), b"prior");
    }
}
