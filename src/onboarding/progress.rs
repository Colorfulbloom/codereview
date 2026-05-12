use rusqlite::Connection;

use super::error::OnboardingError;
use super::state::OnboardingState;

/// Abstraction over storage. Concrete impl uses rusqlite.
pub trait OnboardingPersistence {
    fn load_state(&self) -> Result<Option<OnboardingState>, OnboardingError>;
    fn save_state(&self, state: &OnboardingState) -> Result<(), OnboardingError>;
    fn clear_state(&self) -> Result<(), OnboardingError>;
    fn has_completed_onboarding(&self) -> Result<bool, OnboardingError>;
}

/// SQLite-backed persistence for onboarding state.
pub struct SqliteOnboardingStore<'a> {
    conn: &'a Connection,
}

impl<'a> SqliteOnboardingStore<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }
}

impl OnboardingPersistence for SqliteOnboardingStore<'_> {
    fn load_state(&self) -> Result<Option<OnboardingState>, OnboardingError> {
        let mut stmt = self
            .conn
            .prepare("SELECT state_json FROM onboarding WHERE id = 1")?;

        let result = stmt.query_row([], |row| {
            let json: String = row.get(0)?;
            Ok(json)
        });

        match result {
            Ok(json) => {
                let state: OnboardingState =
                    serde_json::from_str(&json).map_err(|e| OnboardingError::Other(e.into()))?;
                Ok(Some(state))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(OnboardingError::Database(e)),
        }
    }

    fn save_state(&self, state: &OnboardingState) -> Result<(), OnboardingError> {
        let json = serde_json::to_string(state).map_err(|e| OnboardingError::Other(e.into()))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .to_string();

        self.conn.execute(
            "INSERT INTO onboarding (id, state_json, version, updated_at)
             VALUES (1, ?1, 1, ?2)
             ON CONFLICT(id) DO UPDATE SET state_json = ?1, updated_at = ?2",
            rusqlite::params![json, now],
        )?;

        Ok(())
    }

    fn clear_state(&self) -> Result<(), OnboardingError> {
        self.conn
            .execute("DELETE FROM onboarding WHERE id = 1", [])?;
        Ok(())
    }

    fn has_completed_onboarding(&self) -> Result<bool, OnboardingError> {
        match self.load_state()? {
            Some(state) => Ok(state.is_complete()),
            None => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::onboarding::state::{StepData, StepStatus};
    use crate::onboarding::steps::StepId;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE onboarding (
                id         INTEGER PRIMARY KEY CHECK (id = 1),
                state_json TEXT    NOT NULL,
                version    INTEGER NOT NULL DEFAULT 1,
                updated_at TEXT    NOT NULL
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn load_state_returns_none_when_empty() {
        let conn = setup_test_db();
        let store = SqliteOnboardingStore::new(&conn);
        assert!(store.load_state().unwrap().is_none());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let conn = setup_test_db();
        let store = SqliteOnboardingStore::new(&conn);

        let mut state = OnboardingState::default();
        state.record(
            StepId::Welcome,
            StepStatus::Completed,
            Some(StepData::Welcome),
        );
        state.record(StepId::OllamaCheck, StepStatus::Skipped, None);

        store.save_state(&state).unwrap();

        let loaded = store.load_state().unwrap().unwrap();
        assert_eq!(
            loaded.step_status(StepId::Welcome),
            Some(StepStatus::Completed)
        );
        assert_eq!(
            loaded.step_status(StepId::OllamaCheck),
            Some(StepStatus::Skipped)
        );
        assert_eq!(loaded.next_pending(), Some(StepId::ModelSelection));
    }

    #[test]
    fn save_overwrites_previous_state() {
        let conn = setup_test_db();
        let store = SqliteOnboardingStore::new(&conn);

        let mut state1 = OnboardingState::default();
        state1.record(StepId::Welcome, StepStatus::Completed, None);
        store.save_state(&state1).unwrap();

        let mut state2 = OnboardingState::default();
        state2.record(StepId::Welcome, StepStatus::Completed, None);
        state2.record(StepId::OllamaCheck, StepStatus::Completed, None);
        store.save_state(&state2).unwrap();

        let loaded = store.load_state().unwrap().unwrap();
        assert_eq!(
            loaded.step_status(StepId::OllamaCheck),
            Some(StepStatus::Completed)
        );
    }

    #[test]
    fn clear_state_removes_data() {
        let conn = setup_test_db();
        let store = SqliteOnboardingStore::new(&conn);

        let mut state = OnboardingState::default();
        state.record(StepId::Welcome, StepStatus::Completed, None);
        store.save_state(&state).unwrap();

        store.clear_state().unwrap();
        assert!(store.load_state().unwrap().is_none());
    }

    #[test]
    fn has_completed_onboarding_false_when_empty() {
        let conn = setup_test_db();
        let store = SqliteOnboardingStore::new(&conn);
        assert!(!store.has_completed_onboarding().unwrap());
    }

    #[test]
    fn has_completed_onboarding_false_when_partial() {
        let conn = setup_test_db();
        let store = SqliteOnboardingStore::new(&conn);

        let mut state = OnboardingState::default();
        state.record(StepId::Welcome, StepStatus::Completed, None);
        store.save_state(&state).unwrap();

        assert!(!store.has_completed_onboarding().unwrap());
    }

    #[test]
    fn has_completed_onboarding_true_when_all_done() {
        let conn = setup_test_db();
        let store = SqliteOnboardingStore::new(&conn);

        let mut state = OnboardingState::default();
        for step in StepId::all() {
            state.record(*step, StepStatus::Completed, None);
        }
        store.save_state(&state).unwrap();

        assert!(store.has_completed_onboarding().unwrap());
    }
}
