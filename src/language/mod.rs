//! Language detection and rule management.

pub mod rules;

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Supported languages for code review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Php,
    Drupal,
    JavaScript,
    Css,
    Html,
    Yaml,
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Language::Php => write!(f, "PHP"),
            Language::Drupal => write!(f, "Drupal"),
            Language::JavaScript => write!(f, "JavaScript"),
            Language::Css => write!(f, "CSS"),
            Language::Html => write!(f, "HTML"),
            Language::Yaml => write!(f, "YAML"),
        }
    }
}

/// Detect the language of a file from its path.
///
/// Returns `None` for unsupported file types.
pub fn detect_language(path: &str) -> Option<Language> {
    let path = Path::new(path);
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let filename = path.file_name().and_then(|f| f.to_str()).unwrap_or("");

    match ext {
        // Drupal-specific extensions (takes priority over plain PHP)
        "module" | "install" | "theme" | "profile" => Some(Language::Drupal),

        // Drupal info files
        "yml" if filename.ends_with(".info.yml") => Some(Language::Drupal),

        // Twig templates (all treated as HTML)
        "twig" => Some(Language::Html),

        // PHP
        "php" | "inc" => Some(Language::Php),

        // JavaScript
        "js" | "mjs" | "cjs" | "jsx" => Some(Language::JavaScript),

        // TypeScript (reviewed as JS for now)
        "ts" | "tsx" => Some(Language::JavaScript),

        // CSS
        "css" | "scss" | "sass" | "less" => Some(Language::Css),

        // HTML
        "html" | "htm" => Some(Language::Html),

        // YAML (but not Drupal .info.yml which is already caught above)
        "yaml" => Some(Language::Yaml),
        "yml" if !filename.ends_with(".info.yml") => Some(Language::Yaml),

        _ => None,
    }
}

/// Check if a project appears to be a Drupal project.
///
/// Looks for markers like `.info.yml` files, `core/lib/Drupal`, or
/// `composer.json` with `drupal/core` dependency.
pub fn is_drupal_project(file_paths: &[&str]) -> bool {
    file_paths.iter().any(|p| {
        p.ends_with(".info.yml")
            || p.contains("core/lib/Drupal")
            || p.contains("modules/custom/")
            || p.contains("themes/custom/")
            || p.ends_with(".module")
            || p.ends_with(".install")
    })
}

/// Check filesystem markers at a project root for a Drupal installation.
///
/// Complements [`is_drupal_project`], which only sees the reviewed file list:
/// a path review of a single `.php` file carries no Drupal markers of its
/// own, but the project root does (`core/lib/Drupal.php` under a common
/// docroot, or a `composer.json` depending on `drupal/core`).
pub fn is_drupal_project_root(root: &Path) -> bool {
    const DOCROOTS: &[&str] = &["", "web", "docroot", "html"];
    if DOCROOTS
        .iter()
        .any(|d| root.join(d).join("core/lib/Drupal.php").exists())
    {
        return true;
    }

    std::fs::read_to_string(root.join("composer.json"))
        .map(|c| c.contains("drupal/core"))
        .unwrap_or(false)
}

/// Detect all languages present in a set of file paths.
///
/// If Drupal markers are found, `.php` files are promoted to `Drupal`
/// instead of plain `PHP`.
pub fn detect_languages(file_paths: &[&str]) -> BTreeSet<Language> {
    let is_drupal = is_drupal_project(file_paths);

    let mut languages = BTreeSet::new();

    for path in file_paths {
        if let Some(mut lang) = detect_language(path) {
            // In a Drupal project, promote PHP to Drupal
            if is_drupal && lang == Language::Php {
                lang = Language::Drupal;
            }
            languages.insert(lang);
        }
    }

    languages
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- is_drupal_project_root --

    #[test]
    fn drupal_root_detected_at_each_docroot() {
        for docroot in ["", "web", "docroot", "html"] {
            let dir = tempfile::TempDir::new().unwrap();
            let core_lib = dir.path().join(docroot).join("core/lib");
            std::fs::create_dir_all(&core_lib).unwrap();
            std::fs::write(core_lib.join("Drupal.php"), "<?php\n").unwrap();

            assert!(
                is_drupal_project_root(dir.path()),
                "docroot '{docroot}' not detected"
            );
        }
    }

    #[test]
    fn drupal_root_detected_via_composer_json() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("composer.json"),
            r#"{"require": {"drupal/core-recommended": "^11.0"}}"#,
        )
        .unwrap();

        assert!(is_drupal_project_root(dir.path()));
    }

    #[test]
    fn non_drupal_root_not_detected() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("composer.json"),
            r#"{"require": {"laravel/framework": "^11.0"}}"#,
        )
        .unwrap();

        assert!(!is_drupal_project_root(dir.path()));
    }

    // -- detect_language --

    #[test]
    fn detect_php() {
        assert_eq!(detect_language("src/Controller.php"), Some(Language::Php));
        assert_eq!(detect_language("includes/helper.inc"), Some(Language::Php));
    }

    #[test]
    fn detect_drupal_module() {
        assert_eq!(detect_language("mymodule.module"), Some(Language::Drupal));
        assert_eq!(detect_language("mymodule.install"), Some(Language::Drupal));
        assert_eq!(detect_language("mytheme.theme"), Some(Language::Drupal));
        assert_eq!(detect_language("myprofile.profile"), Some(Language::Drupal));
    }

    #[test]
    fn detect_drupal_info_yml() {
        assert_eq!(detect_language("mymodule.info.yml"), Some(Language::Drupal));
    }

    #[test]
    fn detect_javascript() {
        assert_eq!(detect_language("app.js"), Some(Language::JavaScript));
        assert_eq!(detect_language("module.mjs"), Some(Language::JavaScript));
        assert_eq!(detect_language("config.cjs"), Some(Language::JavaScript));
        assert_eq!(detect_language("Component.jsx"), Some(Language::JavaScript));
    }

    #[test]
    fn detect_typescript_as_javascript() {
        assert_eq!(detect_language("app.ts"), Some(Language::JavaScript));
        assert_eq!(detect_language("Component.tsx"), Some(Language::JavaScript));
    }

    #[test]
    fn detect_css() {
        assert_eq!(detect_language("styles.css"), Some(Language::Css));
        assert_eq!(detect_language("main.scss"), Some(Language::Css));
        assert_eq!(detect_language("base.sass"), Some(Language::Css));
        assert_eq!(detect_language("theme.less"), Some(Language::Css));
    }

    #[test]
    fn detect_html() {
        assert_eq!(detect_language("index.html"), Some(Language::Html));
        assert_eq!(detect_language("page.htm"), Some(Language::Html));
    }

    #[test]
    fn detect_twig_as_html() {
        assert_eq!(detect_language("node.html.twig"), Some(Language::Html));
        // Plain .twig files also detected as HTML
        assert_eq!(detect_language("template.twig"), Some(Language::Html));
    }

    #[test]
    fn detect_unknown_returns_none() {
        assert_eq!(detect_language("Cargo.toml"), None);
        assert_eq!(detect_language("README.md"), None);
        assert_eq!(detect_language("image.png"), None);
        assert_eq!(detect_language(".gitignore"), None);
    }

    #[test]
    fn detect_yaml() {
        assert_eq!(detect_language("config.yml"), Some(Language::Yaml));
        assert_eq!(detect_language("docker-compose.yaml"), Some(Language::Yaml));
        assert_eq!(detect_language(".lando.yml"), Some(Language::Yaml));
        assert_eq!(
            detect_language("mymodule.services.yml"),
            Some(Language::Yaml)
        );
        assert_eq!(
            detect_language("mymodule.routing.yml"),
            Some(Language::Yaml)
        );
    }

    #[test]
    fn detect_drupal_info_yml_not_yaml() {
        // .info.yml is Drupal, not generic YAML
        assert_eq!(detect_language("mymodule.info.yml"), Some(Language::Drupal));
    }

    // -- is_drupal_project --

    #[test]
    fn drupal_project_detected_by_info_yml() {
        assert!(is_drupal_project(&[
            "mymodule.info.yml",
            "src/Controller.php"
        ]));
    }

    #[test]
    fn drupal_project_detected_by_module_file() {
        assert!(is_drupal_project(&["mymodule.module", "mymodule.info.yml"]));
    }

    #[test]
    fn drupal_project_detected_by_core_path() {
        assert!(is_drupal_project(&["core/lib/Drupal/Core/Entity.php"]));
    }

    #[test]
    fn drupal_project_detected_by_custom_modules() {
        assert!(is_drupal_project(&["modules/custom/mymod/mymod.module"]));
    }

    #[test]
    fn non_drupal_project() {
        assert!(!is_drupal_project(&[
            "src/main.php",
            "config/routes.php",
            "public/index.php"
        ]));
    }

    // -- detect_languages --

    #[test]
    fn detect_multiple_languages() {
        let files = vec![
            "src/Controller.php",
            "assets/app.js",
            "assets/styles.css",
            "templates/index.html",
        ];
        let langs = detect_languages(&files);
        assert!(langs.contains(&Language::Php));
        assert!(langs.contains(&Language::JavaScript));
        assert!(langs.contains(&Language::Css));
        assert!(langs.contains(&Language::Html));
    }

    #[test]
    fn drupal_project_promotes_php_to_drupal() {
        let files = vec![
            "mymodule.info.yml",
            "mymodule.module",
            "src/Controller.php",
            "assets/app.js",
        ];
        let langs = detect_languages(&files);
        assert!(langs.contains(&Language::Drupal));
        assert!(!langs.contains(&Language::Php)); // PHP promoted to Drupal
        assert!(langs.contains(&Language::JavaScript));
    }

    #[test]
    fn empty_files_returns_empty() {
        let langs = detect_languages(&[]);
        assert!(langs.is_empty());
    }

    #[test]
    fn unknown_files_skipped() {
        let langs = detect_languages(&["Cargo.toml", "README.md", ".gitignore"]);
        assert!(langs.is_empty());
    }

    // -- Language display --

    #[test]
    fn language_display() {
        assert_eq!(Language::Php.to_string(), "PHP");
        assert_eq!(Language::Drupal.to_string(), "Drupal");
        assert_eq!(Language::JavaScript.to_string(), "JavaScript");
        assert_eq!(Language::Css.to_string(), "CSS");
        assert_eq!(Language::Html.to_string(), "HTML");
    }

    #[test]
    fn language_serializes() {
        let json = serde_json::to_string(&Language::Php).unwrap();
        assert_eq!(json, "\"php\"");

        let restored: Language = serde_json::from_str("\"drupal\"").unwrap();
        assert_eq!(restored, Language::Drupal);
    }
}
