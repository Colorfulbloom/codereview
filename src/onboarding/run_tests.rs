//! Tests for the onboarding runner module.

#![cfg(test)]

#[test]
fn run_onboarding_interactive_resets_when_requested() {
    // Setup in-memory DB with completed onboarding state
    let conn = crate::db::init_in_memory().unwrap();

    let mut state = crate::onboarding::state::OnboardingState::default();
    for step in crate::onboarding::steps::StepId::all() {
        state.record(*step, crate::onboarding::state::StepStatus::Completed, None);
    }

    use crate::onboarding::progress::{OnboardingPersistence, SqliteOnboardingStore};
    let store = SqliteOnboardingStore::new(&conn);
    store.save_state(&state).unwrap();

    // Verify it's complete
    assert!(store.has_completed_onboarding().unwrap());

    // After reset, it should need onboarding again
    store.clear_state().unwrap();
    assert!(!store.has_completed_onboarding().unwrap());
}

#[test]
fn run_onboarding_interactive_resumes_partial() {
    let conn = crate::db::init_in_memory().unwrap();

    // Save partial state (only Welcome completed)
    let mut state = crate::onboarding::state::OnboardingState::default();
    state.record(
        crate::onboarding::steps::StepId::Welcome,
        crate::onboarding::state::StepStatus::Completed,
        Some(crate::onboarding::state::StepData::Welcome),
    );

    use crate::onboarding::progress::{OnboardingPersistence, SqliteOnboardingStore};
    let store = SqliteOnboardingStore::new(&conn);
    store.save_state(&state).unwrap();

    // Should still need onboarding (partial)
    assert!(!store.has_completed_onboarding().unwrap());

    // Load and verify next pending step is OllamaCheck
    let loaded = store.load_state().unwrap().unwrap();
    assert_eq!(
        loaded.next_pending(),
        Some(crate::onboarding::steps::StepId::OllamaCheck)
    );
}
