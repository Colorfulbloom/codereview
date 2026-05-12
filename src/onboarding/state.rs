use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::steps::StepId;

/// Data collected from each completed step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StepData {
    Welcome,
    OllamaCheck {
        ollama_version: String,
        was_already_running: bool,
    },
    ModelSelection {
        selected_model: String,
        pulled_new: bool,
    },
    RepoPlatform {
        accounts: Vec<PlatformAccount>,
    },
    Preferences {
        output_format: OutputFormat,
        auto_stage: bool,
    },
    TeamConfig {
        generated_path: Option<PathBuf>,
    },
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformAccount {
    pub platform: Platform,
    pub host: String,
    pub username: String,
    /// Key name in OS keychain — NOT the raw token.
    pub token_ref: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Platform {
    GitHub,
    GitLab,
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Platform::GitHub => write!(f, "GitHub"),
            Platform::GitLab => write!(f, "GitLab"),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum OutputFormat {
    Terminal,
    Json,
    Annotations,
    Report,
}

impl std::fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutputFormat::Terminal => write!(f, "Terminal"),
            OutputFormat::Json => write!(f, "JSON"),
            OutputFormat::Annotations => write!(f, "PR Annotations"),
            OutputFormat::Report => write!(f, "Report"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepEntry {
    pub status: StepStatus,
    pub data: Option<StepData>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum StepStatus {
    Pending,
    Completed,
    Skipped,
}

/// Full onboarding state — the accumulation of all step results.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OnboardingState {
    pub(crate) entries: BTreeMap<StepId, StepEntry>,
}

impl OnboardingState {
    /// First incomplete step, or None if all done.
    pub fn next_pending(&self) -> Option<StepId> {
        StepId::all()
            .iter()
            .find(|id| {
                self.entries
                    .get(id)
                    .is_none_or(|e| e.status == StepStatus::Pending)
            })
            .copied()
    }

    /// Whether all steps are Completed or Skipped.
    pub fn is_complete(&self) -> bool {
        StepId::all().iter().all(|id| {
            self.entries
                .get(id)
                .is_some_and(|e| matches!(e.status, StepStatus::Completed | StepStatus::Skipped))
        })
    }

    /// Get the status of a specific step.
    pub fn step_status(&self, step: StepId) -> Option<StepStatus> {
        self.entries.get(&step).map(|e| e.status)
    }

    /// Read data from a prior step.
    pub fn get_data(&self, step: StepId) -> Option<&StepData> {
        self.entries.get(&step).and_then(|e| e.data.as_ref())
    }

    /// Mark a step as completed or skipped.
    pub fn record(&mut self, step: StepId, status: StepStatus, data: Option<StepData>) {
        let completed_at = if status != StepStatus::Pending {
            Some(unix_timestamp_now())
        } else {
            None
        };
        self.entries.insert(
            step,
            StepEntry {
                status,
                data,
                completed_at,
            },
        );
    }
}

fn unix_timestamp_now() -> String {
    let now = std::time::SystemTime::now();
    let duration = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", duration.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_state_is_not_complete() {
        let state = OnboardingState::default();
        assert!(!state.is_complete());
    }

    #[test]
    fn empty_state_next_pending_is_first_step() {
        let state = OnboardingState::default();
        assert_eq!(state.next_pending(), Some(StepId::Welcome));
    }

    #[test]
    fn record_completed_step() {
        let mut state = OnboardingState::default();
        state.record(
            StepId::Welcome,
            StepStatus::Completed,
            Some(StepData::Welcome),
        );

        assert_eq!(
            state.step_status(StepId::Welcome),
            Some(StepStatus::Completed)
        );
        assert_eq!(state.next_pending(), Some(StepId::OllamaCheck));
    }

    #[test]
    fn record_skipped_step() {
        let mut state = OnboardingState::default();
        state.record(StepId::Welcome, StepStatus::Skipped, None);

        assert_eq!(
            state.step_status(StepId::Welcome),
            Some(StepStatus::Skipped)
        );
        // Skipped steps are not pending
        assert_eq!(state.next_pending(), Some(StepId::OllamaCheck));
    }

    #[test]
    fn all_steps_completed_is_complete() {
        let mut state = OnboardingState::default();
        for step in StepId::all() {
            state.record(*step, StepStatus::Completed, None);
        }
        assert!(state.is_complete());
        assert_eq!(state.next_pending(), None);
    }

    #[test]
    fn mixed_completed_and_skipped_is_complete() {
        let mut state = OnboardingState::default();
        state.record(StepId::Welcome, StepStatus::Completed, None);
        state.record(StepId::OllamaCheck, StepStatus::Skipped, None);
        state.record(StepId::ModelSelection, StepStatus::Completed, None);
        state.record(StepId::RepoPlatform, StepStatus::Skipped, None);
        state.record(StepId::Preferences, StepStatus::Completed, None);
        state.record(StepId::TeamConfig, StepStatus::Skipped, None);
        state.record(StepId::Done, StepStatus::Completed, None);
        assert!(state.is_complete());
    }

    #[test]
    fn partial_completion_is_not_complete() {
        let mut state = OnboardingState::default();
        state.record(StepId::Welcome, StepStatus::Completed, None);
        state.record(StepId::OllamaCheck, StepStatus::Completed, None);
        // Missing the rest
        assert!(!state.is_complete());
        assert_eq!(state.next_pending(), Some(StepId::ModelSelection));
    }

    #[test]
    fn get_data_returns_none_for_missing_step() {
        let state = OnboardingState::default();
        assert!(state.get_data(StepId::Welcome).is_none());
    }

    #[test]
    fn get_data_returns_stored_data() {
        let mut state = OnboardingState::default();
        state.record(
            StepId::ModelSelection,
            StepStatus::Completed,
            Some(StepData::ModelSelection {
                selected_model: "gemma4".to_string(),
                pulled_new: false,
            }),
        );

        match state.get_data(StepId::ModelSelection) {
            Some(StepData::ModelSelection {
                selected_model,
                pulled_new,
            }) => {
                assert_eq!(selected_model, "gemma4");
                assert!(!pulled_new);
            }
            other => panic!("Expected ModelSelection data, got {other:?}"),
        }
    }

    #[test]
    fn step_status_returns_none_for_unrecorded() {
        let state = OnboardingState::default();
        assert_eq!(state.step_status(StepId::Done), None);
    }

    #[test]
    fn record_sets_completed_at_for_non_pending() {
        let mut state = OnboardingState::default();
        state.record(StepId::Welcome, StepStatus::Completed, None);
        let entry = state.entries.get(&StepId::Welcome).unwrap();
        assert!(entry.completed_at.is_some());
    }

    #[test]
    fn record_does_not_set_completed_at_for_pending() {
        let mut state = OnboardingState::default();
        state.record(StepId::Welcome, StepStatus::Pending, None);
        let entry = state.entries.get(&StepId::Welcome).unwrap();
        assert!(entry.completed_at.is_none());
    }

    #[test]
    fn state_serializes_and_deserializes() {
        let mut state = OnboardingState::default();
        state.record(
            StepId::Welcome,
            StepStatus::Completed,
            Some(StepData::Welcome),
        );
        state.record(StepId::OllamaCheck, StepStatus::Skipped, None);

        let json = serde_json::to_string(&state).unwrap();
        let restored: OnboardingState = serde_json::from_str(&json).unwrap();

        assert_eq!(
            restored.step_status(StepId::Welcome),
            Some(StepStatus::Completed)
        );
        assert_eq!(
            restored.step_status(StepId::OllamaCheck),
            Some(StepStatus::Skipped)
        );
        assert_eq!(restored.next_pending(), Some(StepId::ModelSelection));
    }
}

#[cfg(test)]
mod step_id_tests {
    use super::*;

    #[test]
    fn all_returns_seven_steps() {
        assert_eq!(StepId::all().len(), 7);
    }

    #[test]
    fn total_is_seven() {
        assert_eq!(StepId::total(), 7);
    }

    #[test]
    fn first_step_is_welcome() {
        assert_eq!(StepId::all()[0], StepId::Welcome);
    }

    #[test]
    fn last_step_is_done() {
        assert_eq!(*StepId::all().last().unwrap(), StepId::Done);
    }

    #[test]
    fn next_from_welcome_is_ollama_check() {
        assert_eq!(StepId::Welcome.next(), Some(StepId::OllamaCheck));
    }

    #[test]
    fn next_from_done_is_none() {
        assert_eq!(StepId::Done.next(), None);
    }

    #[test]
    fn number_is_one_indexed() {
        assert_eq!(StepId::Welcome.number(), 1);
        assert_eq!(StepId::Done.number(), 7);
    }

    #[test]
    fn steps_are_in_order() {
        let all = StepId::all();
        for i in 0..all.len() - 1 {
            assert_eq!(all[i].next(), Some(all[i + 1]));
        }
    }
}
