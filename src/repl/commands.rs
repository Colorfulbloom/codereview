/// Registry of available slash commands.
pub struct CommandRegistry {
    sections: Vec<CommandSection>,
}

struct CommandSection {
    name: &'static str,
    commands: Vec<SlashCommandEntry>,
}

struct SlashCommandEntry {
    name: &'static str,
    description: &'static str,
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandRegistry {
    pub fn new() -> Self {
        let sections = vec![
            CommandSection {
                name: "Review",
                commands: vec![
                    SlashCommandEntry {
                        name: "/review",
                        description: "Run a code review on your changes",
                    },
                    SlashCommandEntry {
                        name: "/diff",
                        description: "View the current diff without reviewing",
                    },
                    SlashCommandEntry {
                        name: "/rules",
                        description: "Inspect active review rules per language",
                    },
                    SlashCommandEntry {
                        name: "/commit",
                        description: "Stage and commit reviewed files",
                    },
                ],
            },
            CommandSection {
                name: "Configuration",
                commands: vec![
                    SlashCommandEntry {
                        name: "/config",
                        description: "View or modify configuration",
                    },
                    SlashCommandEntry {
                        name: "/output",
                        description: "Set output format (terminal, json, annotations, report)",
                    },
                    SlashCommandEntry {
                        name: "/models",
                        description: "List, switch, or pull Ollama models",
                    },
                    SlashCommandEntry {
                        name: "/onboard",
                        description: "Re-run the onboarding wizard",
                    },
                    SlashCommandEntry {
                        name: "/init",
                        description: "Generate a .codereview.yaml for your project",
                    },
                ],
            },
            CommandSection {
                name: "Session",
                commands: vec![
                    SlashCommandEntry {
                        name: "/status",
                        description: "Show what's changed and what's been reviewed",
                    },
                    SlashCommandEntry {
                        name: "/debug",
                        description: "Show diagnostic info (git, Ollama, config, languages)",
                    },
                    SlashCommandEntry {
                        name: "/help",
                        description: "Show this help message",
                    },
                    SlashCommandEntry {
                        name: "/quit",
                        description: "Exit the REPL",
                    },
                ],
            },
        ];

        Self { sections }
    }

    pub fn print_help(&self) {
        use console::Style;
        let section_style = Style::new().bold().yellow();
        let cmd_style = Style::new().cyan().bold();
        let desc_style = Style::new().white();

        println!();
        for section in &self.sections {
            println!("  {}:", section_style.apply_to(section.name));
            for cmd in &section.commands {
                println!(
                    "    {:<16}  {}",
                    cmd_style.apply_to(cmd.name),
                    desc_style.apply_to(cmd.description)
                );
            }
            println!();
        }
    }

    /// Return all command names for tab completion.
    pub fn command_names(&self) -> Vec<&'static str> {
        self.sections
            .iter()
            .flat_map(|s| s.commands.iter().map(|c| c.name))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_all_commands() {
        let registry = CommandRegistry::new();
        let names = registry.command_names();

        let expected = [
            "/review", "/diff", "/rules", "/commit", "/config", "/output", "/models", "/onboard",
            "/init", "/status", "/debug", "/help", "/quit",
        ];
        for cmd in &expected {
            assert!(names.contains(cmd), "Missing command: {cmd}");
        }
    }

    #[test]
    fn registry_has_three_sections() {
        let registry = CommandRegistry::new();
        assert_eq!(registry.sections.len(), 3);
        assert_eq!(registry.sections[0].name, "Review");
        assert_eq!(registry.sections[1].name, "Configuration");
        assert_eq!(registry.sections[2].name, "Session");
    }

    #[test]
    fn command_names_all_start_with_slash() {
        let registry = CommandRegistry::new();
        for name in registry.command_names() {
            assert!(name.starts_with('/'), "Command missing slash: {name}");
        }
    }

    #[test]
    fn default_same_as_new() {
        let from_new = CommandRegistry::new().command_names();
        let from_default = CommandRegistry::default().command_names();
        assert_eq!(from_new, from_default);
    }

    #[test]
    fn command_names_count() {
        let registry = CommandRegistry::new();
        assert_eq!(registry.command_names().len(), 13);
    }

    #[test]
    fn no_duplicate_commands() {
        let registry = CommandRegistry::new();
        let names = registry.command_names();
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "Duplicate command names found");
    }
}
