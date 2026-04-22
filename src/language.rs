#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Python,
    Rust,
    JavaScript,
    TypeScript,
    Dockerfile,
    Unknown,
}

impl Language {
    pub fn from_filename(filename: &str) -> Self {
        if filename.ends_with(".py") {
            Language::Python
        } else if filename.ends_with(".rs") {
            Language::Rust
        } else if filename.ends_with(".js") {
            Language::JavaScript
        } else if filename.ends_with(".ts") {
            Language::TypeScript
        } else if filename == "Dockerfile" || filename.ends_with(".Dockerfile") {
            Language::Dockerfile
        } else {
            Language::Unknown
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Language::Python => "python",
            Language::Rust => "rust",
            Language::JavaScript => "javascript",
            Language::TypeScript => "typescript",
            Language::Dockerfile => "dockerfile",
            Language::Unknown => "unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_detection() {
        assert_eq!(Language::from_filename("app.py"), Language::Python);
        assert_eq!(Language::from_filename("main.rs"), Language::Rust);
        assert_eq!(Language::from_filename("index.js"), Language::JavaScript);
        assert_eq!(Language::from_filename("utils.ts"), Language::TypeScript);
        assert_eq!(Language::from_filename("Dockerfile"), Language::Dockerfile);
        assert_eq!(
            Language::from_filename("custom.Dockerfile"),
            Language::Dockerfile
        );
        assert_eq!(Language::from_filename("unknown.txt"), Language::Unknown);
    }
}
