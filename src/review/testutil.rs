//! Test utilities for the review pipeline.

#![cfg(test)]

use std::sync::Mutex;

use async_trait::async_trait;

use crate::onboarding::error::OnboardingError;
use crate::onboarding::steps::OllamaClient;

/// Mock OllamaClient that returns a pre-configured response and captures prompts.
pub struct MockOllama {
    response: String,
    /// All system prompts sent to this mock.
    pub captured_system: Mutex<Vec<String>>,
    /// All user prompts sent to this mock.
    pub captured_user: Mutex<Vec<String>>,
}

impl MockOllama {
    pub fn with_response(json: &str) -> Self {
        Self {
            response: json.to_string(),
            captured_system: Mutex::new(Vec::new()),
            captured_user: Mutex::new(Vec::new()),
        }
    }

    /// Check if any captured system prompt contains a substring.
    pub fn system_prompt_contains(&self, substr: &str) -> bool {
        self.captured_system
            .lock()
            .unwrap()
            .iter()
            .any(|s| s.contains(substr))
    }

    /// Number of chat() calls made.
    pub fn call_count(&self) -> usize {
        self.captured_system.lock().unwrap().len()
    }
}

#[async_trait]
impl OllamaClient for MockOllama {
    fn is_installed(&self) -> bool {
        true
    }
    async fn is_running(&self) -> bool {
        true
    }
    async fn start(&self) -> Result<(), OnboardingError> {
        Ok(())
    }
    async fn version(&self) -> Result<String, OnboardingError> {
        Ok("mock".into())
    }
    async fn list_models(&self) -> Result<Vec<String>, OnboardingError> {
        Ok(vec![])
    }
    async fn pull_model(&self, _: &str) -> Result<(), OnboardingError> {
        Ok(())
    }
    async fn chat(
        &self,
        _model: &str,
        system: &str,
        user: &str,
    ) -> Result<String, OnboardingError> {
        self.captured_system
            .lock()
            .unwrap()
            .push(system.to_string());
        self.captured_user.lock().unwrap().push(user.to_string());
        Ok(self.response.clone())
    }
}

/// Mock that returns different responses on successive calls.
pub struct SequentialMockOllama {
    responses: Mutex<Vec<String>>,
    pub captured_system: Mutex<Vec<String>>,
}

impl SequentialMockOllama {
    pub fn with_responses(responses: Vec<&str>) -> Self {
        Self {
            responses: Mutex::new(responses.into_iter().map(String::from).collect()),
            captured_system: Mutex::new(Vec::new()),
        }
    }

    pub fn call_count(&self) -> usize {
        self.captured_system.lock().unwrap().len()
    }
}

#[async_trait]
impl OllamaClient for SequentialMockOllama {
    fn is_installed(&self) -> bool {
        true
    }
    async fn is_running(&self) -> bool {
        true
    }
    async fn start(&self) -> Result<(), OnboardingError> {
        Ok(())
    }
    async fn version(&self) -> Result<String, OnboardingError> {
        Ok("mock".into())
    }
    async fn list_models(&self) -> Result<Vec<String>, OnboardingError> {
        Ok(vec![])
    }
    async fn pull_model(&self, _: &str) -> Result<(), OnboardingError> {
        Ok(())
    }
    async fn chat(
        &self,
        _model: &str,
        system: &str,
        _user: &str,
    ) -> Result<String, OnboardingError> {
        self.captured_system
            .lock()
            .unwrap()
            .push(system.to_string());
        let mut responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            // Extra agent calls beyond expected — return empty findings
            Ok("[]".to_string())
        } else {
            Ok(responses.remove(0))
        }
    }
}
