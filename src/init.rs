//! Interactive config file generator for .codereview.yaml.

use std::path::Path;

use anyhow::Result;
use console::Style;
use dialoguer::{Confirm, Input, MultiSelect, Select};

use crate::language::Language;
use crate::language::rules::builtin_rules;

/// Run the interactive init wizard to generate .codereview.yaml.
pub fn run_init() -> Result<()> {
    let config_path = Path::new(".codereview.yaml");

    if config_path.exists() {
        let overwrite = Confirm::new()
            .with_prompt(".codereview.yaml already exists. Overwrite?")
            .default(false)
            .interact()?;
        if !overwrite {
            println!("Cancelled.");
            return Ok(());
        }
    }

    let bold = Style::new().bold();
    let dim = Style::new().dim();

    println!(
        "\n{}",
        bold.apply_to("code-review init — generate .codereview.yaml")
    );
    println!(
        "{}",
        dim.apply_to("This wizard helps you create a configuration file for your project.\n")
    );

    let mut yaml = template_header();

    // 1. Model selection — existing models first, then hardware-based recommendations
    println!("{}", bold.apply_to("Step 1: Model"));
    select_model(&mut yaml, &bold, &dim)?;

    // 2. Performance (per-request LLM timeout)
    println!("\n{}", bold.apply_to("Step 2: Performance"));
    select_timeout(&mut yaml, &dim)?;

    // 3. Languages
    println!("\n{}", bold.apply_to("Step 3: Languages"));
    select_languages(&mut yaml)?;

    // 4. Rule overrides
    println!("\n{}", bold.apply_to("Step 4: Rule Overrides"));
    configure_rules(&mut yaml)?;

    // 5. Custom rules
    println!("\n{}", bold.apply_to("Step 5: Custom Rules"));
    add_custom_rules(&mut yaml)?;

    // 6. Custom agents
    println!("\n{}", bold.apply_to("Step 6: Custom Agents"));
    println!(
        "{}",
        dim.apply_to(
            "Custom agents are specialized reviewers with their own system prompts.\n\
             Examples: PCI-DSS compliance, Laravel conventions, performance analysis."
        )
    );
    add_custom_agents(&mut yaml)?;

    // Write the file
    std::fs::write(config_path, &yaml)?;

    println!("\n{}", bold.apply_to("Created: .codereview.yaml"));
    println!("Run /config in the REPL to verify, or /rules to see active rules.");
    println!(
        "{}",
        dim.apply_to("Edit the file anytime — see docs/CONFIGURATION.md for all options.")
    );

    Ok(())
}

// ── Step 1: Model Selection ──

fn select_model(yaml: &mut String, bold: &Style, dim: &Style) -> Result<()> {
    // Detect hardware
    let hw = detect_hardware();
    println!(
        "{}",
        dim.apply_to(format!(
            "  Detected: {} RAM{}",
            format_gb(hw.total_ram_gb),
            hw.chip_name
                .as_ref()
                .map(|c| format!(", {c}"))
                .unwrap_or_default()
        ))
    );

    // Query Ollama for existing models
    let existing_models = query_ollama_models();

    let mut options: Vec<String> = Vec::new();
    let mut model_values: Vec<Option<String>> = Vec::new(); // None = skip

    // Section 1: Existing models (if any)
    if !existing_models.is_empty() {
        println!(
            "{}",
            dim.apply_to(format!(
                "  Found {} model(s) in Ollama\n",
                existing_models.len()
            ))
        );
        for m in &existing_models {
            options.push(format!("{m} (installed)"));
            model_values.push(Some(m.clone()));
        }
        options.push("───── Recommended for your hardware ─────".to_string());
        model_values.push(None); // separator
    }

    // Section 2: Hardware-based recommendations
    let recommendations = recommend_models(hw.total_ram_gb);
    for (name, ram, desc) in &recommendations {
        // Skip if already installed
        if existing_models.iter().any(|m| m.starts_with(name)) {
            continue;
        }
        options.push(format!("{name} (~{ram}GB RAM — {desc})"));
        model_values.push(Some(name.to_string()));
    }

    // Section 3: Custom + skip
    options.push("Custom model name".to_string());
    model_values.push(None);
    options.push("Skip (use default from onboarding)".to_string());
    model_values.push(None);

    let model_idx = Select::new()
        .with_prompt("Default model for reviews")
        .items(&options)
        .default(0)
        .interact()?;

    let is_last = model_idx == options.len() - 1;
    let is_custom = model_idx == options.len() - 2;
    let is_separator = model_values[model_idx].is_none() && !is_last && !is_custom;

    if is_separator {
        println!(
            "{}",
            bold.apply_to("Please select a model, not the separator.")
        );
        println!("{}", dim.apply_to(""));
        return select_model(yaml, bold, dim);
    } else if is_last {
        yaml.push_str("# model: gemma4  # uncomment to override\n\n");
    } else if is_custom {
        let name: String = Input::new().with_prompt("Model name").interact_text()?;
        yaml.push_str(&format!("model: {name}\n\n"));
    } else if let Some(ref model) = model_values[model_idx] {
        yaml.push_str(&format!("model: {model}\n\n"));
    }

    Ok(())
}

/// Hardware info detected from the system.
struct HardwareInfo {
    total_ram_gb: u64,
    chip_name: Option<String>,
}

fn detect_hardware() -> HardwareInfo {
    use sysinfo::System;

    let sys = System::new_all();
    let total_ram_gb = sys.total_memory() / (1024 * 1024 * 1024);

    let chip_name = if cfg!(target_os = "macos") {
        get_macos_chip_name()
    } else {
        None
    };

    HardwareInfo {
        total_ram_gb,
        chip_name,
    }
}

#[cfg(target_os = "macos")]
fn get_macos_chip_name() -> Option<String> {
    use std::process::Command;
    let output = Command::new("sysctl")
        .args(["-n", "machdep.cpu.brand_string"])
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

#[cfg(not(target_os = "macos"))]
fn get_macos_chip_name() -> Option<String> {
    None
}

/// Query Ollama for locally installed models.
fn query_ollama_models() -> Vec<String> {
    let rt = tokio::runtime::Runtime::new().ok();
    let rt = match rt {
        Some(rt) => rt,
        None => return vec![],
    };

    rt.block_on(async {
        let resp = reqwest::get("http://127.0.0.1:11434/api/tags").await.ok();
        let resp = match resp {
            Some(r) => r,
            None => return vec![],
        };

        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        body["models"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m["name"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// Recommend models based on available RAM.
/// Returns: (model_name, ram_needed_gb, description)
fn recommend_models(ram_gb: u64) -> Vec<(&'static str, u64, &'static str)> {
    match ram_gb {
        0..=8 => vec![
            ("qwen3:4b", 4, "lightweight, fast responses"),
            ("phi4-mini", 4, "strong reasoning for its size"),
            ("deepseek-r1:1.5b", 2, "chain-of-thought, ultra-light"),
        ],
        9..=15 => vec![
            ("qwen3:8b", 6, "best 8B coder"),
            ("qwen2.5-coder:14b", 10, "strong code specialist"),
            ("gemma3:12b", 9, "balanced quality and speed"),
            ("deepseek-r1:8b", 6, "chain-of-thought debugging"),
        ],
        16..=31 => vec![
            ("qwen3-coder:30b", 20, "best quality, 256K context"),
            ("qwen3:32b", 22, "best dense coding model"),
            ("devstral:24b", 15, "agentic coding specialist"),
            ("qwen2.5-coder:14b", 10, "faster, still accurate"),
        ],
        32..=63 => vec![
            ("qwen3-coder:30b", 20, "best quality, 256K context"),
            ("qwen3:32b", 22, "best dense coding model"),
            ("llama3.3:70b", 48, "excellent all-rounder"),
            ("deepseek-r1:32b", 22, "deep reasoning + code"),
        ],
        _ => vec![
            ("llama3.3:70b", 48, "excellent all-rounder"),
            ("qwen3:72b", 48, "top-tier dense model"),
            ("qwen3-coder:30b", 20, "best code quality"),
            ("deepseek-r1:70b", 48, "best reasoning at 70B"),
        ],
    }
}

fn format_gb(gb: u64) -> String {
    format!("{gb}GB")
}

/// The fixed top of every generated `.codereview.yaml`: provenance comments
/// plus the context-window hint, commented out so changing it is an
/// uncomment-and-edit. The timeout gets its own wizard step.
fn template_header() -> String {
    let mut yaml = String::new();
    yaml.push_str("# code-review configuration\n");
    yaml.push_str("# Generated by: code-review init\n");
    yaml.push_str("# Documentation: see docs/CONFIGURATION.md\n\n");
    yaml.push_str("# Context window (tokens) per request; auto-detected and capped by the model.\n");
    yaml.push_str("# Lower it on low-RAM machines for smaller, faster requests.\n");
    yaml.push_str("# max_context_tokens: 16384\n\n");
    yaml.push_str("# Anti-hallucination second pass (opt-in). Re-checks each bug/security\n");
    yaml.push_str("# finding against its code and drops the ones that misread it. Adds one\n");
    yaml.push_str("# LLM call per in-scope finding. Also available per-run: --verify.\n");
    yaml.push_str("# verify: true\n\n");
    yaml.push_str("# PHP_CodeSniffer (Drupal coding standards) as the deterministic source of\n");
    yaml.push_str("# truth for rule-based Drupal/PHP checks (dependency injection, coding\n");
    yaml.push_str("# standards). Auto-runs when phpcs + the Drupal standard are installed. When\n");
    yaml.push_str("# PHP runs in a container (DDEV/Lando/Docker), point `command` at it:\n");
    yaml.push_str("# phpcs:\n");
    yaml.push_str("#   command: \"lando phpcs\"   # or \"ddev exec phpcs\"\n\n");
    yaml.push_str("# ESLint + Stylelint as the deterministic source of truth for the mechanical\n");
    yaml.push_str("# JS/CSS rules (var, ===, !important, etc.) — the JS/CSS analog of phpcs.\n");
    yaml.push_str("# Auto-run when installed AND a project config resolves. PHP-less? Still works:\n");
    yaml.push_str("# they run through node. In a container, point `command` at it:\n");
    yaml.push_str("# eslint:\n");
    yaml.push_str("#   command: \"lando eslint\"\n");
    yaml.push_str("# stylelint:\n");
    yaml.push_str("#   command: \"lando stylelint\"\n\n");
    yaml
}

// ── Step 2: Performance ──

fn select_timeout(yaml: &mut String, dim: &Style) -> Result<()> {
    println!(
        "{}",
        dim.apply_to(
            "  How long a single LLM request may run. Slow hardware needs more time;\n  \
             0 disables the timeout (a stalled Ollama will hang the review)."
        )
    );

    let secs: u64 = Input::new()
        .with_prompt("Per-request LLM timeout in seconds")
        .default(300)
        .interact_text()?;

    yaml.push_str(&timeout_yaml_block(secs));
    Ok(())
}

/// YAML block for the chosen timeout: an active line when it differs from the
/// 300s default, a commented line otherwise — so an untouched default doesn't
/// pin the config to a value the tool would use anyway.
fn timeout_yaml_block(secs: u64) -> String {
    let mut block = String::new();
    block.push_str("# Per-LLM-request timeout in seconds. Default: 300 when unset.\n");
    block.push_str("# 0 = no timeout — a stalled Ollama will hang the review.\n");
    if secs == 300 {
        block.push_str("# llm_timeout_seconds: 300\n\n");
    } else {
        block.push_str(&format!("llm_timeout_seconds: {secs}\n\n"));
    }
    block
}

// ── Step 3: Languages ──

fn select_languages(yaml: &mut String) -> Result<()> {
    let lang_options = vec!["PHP", "Drupal", "JavaScript", "CSS", "HTML"];
    let lang_keys = ["php", "drupal", "javascript", "css", "html"];

    let auto_detect = Confirm::new()
        .with_prompt("Auto-detect languages from file extensions?")
        .default(true)
        .interact()?;

    if !auto_detect {
        let selected = MultiSelect::new()
            .with_prompt("Select languages to review")
            .items(&lang_options)
            .interact()?;

        if !selected.is_empty() {
            yaml.push_str("languages:\n");
            for &idx in &selected {
                yaml.push_str(&format!("  - {}\n", lang_keys[idx]));
            }
            yaml.push('\n');
        }
    } else {
        yaml.push_str("# languages: auto-detected from file extensions\n\n");
    }

    Ok(())
}

// ── Step 4: Rule Overrides ──

fn configure_rules(yaml: &mut String) -> Result<()> {
    let customize_rules = Confirm::new()
        .with_prompt("Customize any built-in rules? (disable or change severity)")
        .default(false)
        .interact()?;

    if !customize_rules {
        return Ok(());
    }

    yaml.push_str("rules:\n");

    let languages_to_configure = vec![
        (Language::Php, "php"),
        (Language::Drupal, "drupal"),
        (Language::JavaScript, "javascript"),
        (Language::Css, "css"),
        (Language::Html, "html"),
    ];

    for (lang, key) in &languages_to_configure {
        let rules = builtin_rules(*lang);
        let rule_names: Vec<String> = rules
            .iter()
            .map(|r| format!("[{}] {} — {}", r.severity, r.id, r.description))
            .collect();

        let configure_lang = Confirm::new()
            .with_prompt(format!("Configure {} rules?", lang))
            .default(false)
            .interact()?;

        if !configure_lang {
            continue;
        }

        let to_disable = MultiSelect::new()
            .with_prompt(format!("Select {} rules to DISABLE", lang))
            .items(&rule_names)
            .interact()?;

        if !to_disable.is_empty() {
            yaml.push_str(&format!("  {key}:\n"));
            for &idx in &to_disable {
                yaml.push_str(&format!("    {}:\n", rules[idx].id));
                yaml.push_str("      enabled: false\n");
            }
        }
    }
    yaml.push('\n');

    Ok(())
}

// ── Step 5: Custom Rules ──

fn add_custom_rules(yaml: &mut String) -> Result<()> {
    let lang_options = vec!["PHP", "Drupal", "JavaScript", "CSS", "HTML"];
    let lang_keys = ["php", "drupal", "javascript", "css", "html"];

    let add_custom = Confirm::new()
        .with_prompt("Add custom review rules?")
        .default(false)
        .interact()?;

    if !add_custom {
        return Ok(());
    }

    yaml.push_str("custom_rules:\n");
    loop {
        let id: String = Input::new()
            .with_prompt("Rule ID (e.g., no-debug-code)")
            .interact_text()?;

        let description: String = Input::new()
            .with_prompt("Description (what to check for)")
            .interact_text()?;

        let severity_options = vec!["error", "warning", "info"];
        let severity_idx = Select::new()
            .with_prompt("Severity")
            .items(&severity_options)
            .default(1)
            .interact()?;

        let all_langs = Confirm::new()
            .with_prompt("Apply to all languages?")
            .default(true)
            .interact()?;

        yaml.push_str(&format!("  - id: {id}\n"));
        yaml.push_str(&format!("    description: \"{description}\"\n"));
        yaml.push_str(&format!(
            "    severity: {}\n",
            severity_options[severity_idx]
        ));

        if !all_langs {
            let selected = MultiSelect::new()
                .with_prompt("Select languages")
                .items(&lang_options)
                .interact()?;
            if !selected.is_empty() {
                let langs: Vec<&str> = selected.iter().map(|&i| lang_keys[i]).collect();
                yaml.push_str(&format!("    languages: [{}]\n", langs.join(", ")));
            }
        }

        let more = Confirm::new()
            .with_prompt("Add another custom rule?")
            .default(false)
            .interact()?;
        if !more {
            break;
        }
    }
    yaml.push('\n');

    Ok(())
}

// ── Step 6: Custom Agents ──

fn add_custom_agents(yaml: &mut String) -> Result<()> {
    let lang_options = vec!["PHP", "Drupal", "JavaScript", "CSS", "HTML"];
    let lang_keys = ["php", "drupal", "javascript", "css", "html"];

    let add_agents = Confirm::new()
        .with_prompt("Add custom review agents?")
        .default(false)
        .interact()?;

    if !add_agents {
        return Ok(());
    }

    yaml.push_str("agents:\n");
    loop {
        let name: String = Input::new()
            .with_prompt("Agent name (e.g., \"PCI-DSS Compliance\")")
            .interact_text()?;

        let prompt: String = Input::new()
            .with_prompt("System prompt (what this agent should focus on)")
            .interact_text()?;

        let all_langs = Confirm::new()
            .with_prompt("Run on all languages?")
            .default(true)
            .interact()?;

        yaml.push_str(&format!("  - name: \"{name}\"\n"));
        yaml.push_str("    prompt: |\n");
        for line in prompt.lines() {
            yaml.push_str(&format!("      {line}\n"));
        }

        if !all_langs {
            let selected = MultiSelect::new()
                .with_prompt("Select languages")
                .items(&lang_options)
                .interact()?;
            if !selected.is_empty() {
                let langs: Vec<&str> = selected.iter().map(|&i| lang_keys[i]).collect();
                yaml.push_str(&format!("    languages: [{}]\n", langs.join(", ")));
            }
        }

        let add_rules = Confirm::new()
            .with_prompt("Add rules for this agent?")
            .default(false)
            .interact()?;

        if add_rules {
            yaml.push_str("    rules:\n");
            loop {
                let rule_id: String = Input::new().with_prompt("Rule ID").interact_text()?;
                let rule_desc: String = Input::new().with_prompt("Description").interact_text()?;
                let sev_options = vec!["error", "warning", "info"];
                let sev_idx = Select::new()
                    .with_prompt("Severity")
                    .items(&sev_options)
                    .default(1)
                    .interact()?;

                yaml.push_str(&format!("      - id: {rule_id}\n"));
                yaml.push_str(&format!("        description: \"{rule_desc}\"\n"));
                yaml.push_str(&format!("        severity: {}\n", sev_options[sev_idx]));

                let more_rules = Confirm::new()
                    .with_prompt("Add another rule?")
                    .default(false)
                    .interact()?;
                if !more_rules {
                    break;
                }
            }
        }

        let more_agents = Confirm::new()
            .with_prompt("Add another agent?")
            .default(false)
            .interact()?;
        if !more_agents {
            break;
        }
    }
    yaml.push('\n');

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Hardware detection ──

    #[test]
    fn template_header_is_valid_yaml_with_context_hint() {
        let header = template_header();
        assert!(header.contains("# max_context_tokens:"));
        // The opt-in anti-hallucination second pass is hinted (commented).
        assert!(header.contains("# verify:"));
        // phpcs (deterministic Drupal/PHP source of truth) is hinted (commented).
        assert!(header.contains("# phpcs:"));
        // ESLint/Stylelint (deterministic JS/CSS source of truth) are hinted.
        assert!(header.contains("# eslint:"));
        assert!(header.contains("# stylelint:"));
        // The header alone must be a valid (all-comment) config.
        assert!(crate::config::Config::parse(&header).is_ok());
    }

    #[test]
    fn timeout_yaml_block_active_when_nondefault() {
        let block = timeout_yaml_block(900);
        let config = crate::config::Config::parse(&block).unwrap();
        assert_eq!(config.llm_timeout_seconds, Some(900));
        assert_eq!(config.llm_timeout(), 900);
    }

    #[test]
    fn timeout_yaml_block_commented_at_default() {
        let block = timeout_yaml_block(300);
        assert!(block.contains("# llm_timeout_seconds: 300"));
        let config = crate::config::Config::parse(&block).unwrap();
        assert_eq!(config.llm_timeout_seconds, None);
    }

    #[test]
    fn timeout_yaml_block_zero_is_unlimited() {
        let block = timeout_yaml_block(0);
        assert!(block.contains("0 = no timeout"));
        let config = crate::config::Config::parse(&block).unwrap();
        assert_eq!(config.llm_timeout_seconds, Some(0));
    }

    #[test]
    fn detect_hardware_returns_nonzero_ram() {
        let hw = detect_hardware();
        assert!(
            hw.total_ram_gb > 0,
            "RAM should be > 0, got {}",
            hw.total_ram_gb
        );
    }

    #[test]
    fn format_gb_works() {
        assert_eq!(format_gb(16), "16GB");
        assert_eq!(format_gb(0), "0GB");
        assert_eq!(format_gb(128), "128GB");
    }

    // ── Model recommendations ──

    #[test]
    fn recommend_models_8gb() {
        let models = recommend_models(8);
        assert!(!models.is_empty());
        // At least one model should comfortably fit
        assert!(
            models.iter().any(|(_, ram, _)| *ram <= 8),
            "No model fits in 8GB"
        );
    }

    #[test]
    fn recommend_models_16gb() {
        let models = recommend_models(16);
        assert!(!models.is_empty());
        // At least one model should comfortably fit
        assert!(
            models.iter().any(|(_, ram, _)| *ram <= 16),
            "No model fits in 16GB"
        );
    }

    #[test]
    fn recommend_models_32gb() {
        let models = recommend_models(32);
        assert!(!models.is_empty());
        assert!(models.iter().any(|(_, ram, _)| *ram >= 15));
    }

    #[test]
    fn recommend_models_64gb() {
        let models = recommend_models(64);
        assert!(!models.is_empty());
        assert!(models.iter().any(|(_, ram, _)| *ram >= 40));
    }

    #[test]
    fn recommend_models_all_have_names() {
        for ram in [4, 8, 16, 32, 64, 128] {
            let models = recommend_models(ram);
            for (name, _, desc) in &models {
                assert!(!name.is_empty(), "Model name empty for {ram}GB tier");
                assert!(!desc.is_empty(), "Model desc empty for {ram}GB tier");
            }
        }
    }

    #[test]
    fn recommend_models_no_duplicates() {
        for ram in [8, 16, 32, 64, 128] {
            let models = recommend_models(ram);
            let names: Vec<&str> = models.iter().map(|(n, _, _)| *n).collect();
            let mut unique = names.clone();
            unique.sort();
            unique.dedup();
            assert_eq!(
                names.len(),
                unique.len(),
                "Duplicate models in {ram}GB tier"
            );
        }
    }

    // ── Ollama query ──

    #[test]
    fn query_ollama_models_returns_vec() {
        // May return empty if Ollama isn't running — that's OK
        let models = query_ollama_models();
        let _ = models; // just verify no panic
    }

    // ── macOS chip detection ──

    #[test]
    fn get_chip_name_does_not_panic() {
        let _chip = get_macos_chip_name();
    }
}
