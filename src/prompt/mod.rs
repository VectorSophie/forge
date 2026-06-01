use crate::language::Language;

/// Returns (system_message, user_message) for chat APIs.
pub fn build_prompt(language: Language, code: &str) -> (String, String) {
    let lang_hint = match language {
        Language::Python => "Python. Follow PEP8. Keep all existing function signatures and types exactly as-is.",
        Language::Rust => "Rust. Keep all existing function signatures, types, and trait impls exactly as-is. Use idiomatic Result, ownership, and match.",
        Language::JavaScript => "JavaScript ES6+. Keep all existing function signatures exactly as-is.",
        Language::TypeScript => "TypeScript. Keep all existing function signatures and type annotations exactly as-is.",
        Language::Dockerfile => "Dockerfile. Preserve all existing directives and layering.",
        Language::Unknown => "the language shown in the file.",
    };

    let system = format!(
        "You are a code-completion engine. Rules:\n\
         1. Output ONLY the completed source file. No markdown fences, no explanations, no commentary.\n\
         2. Fill in TODO, pass, todo!(), unimplemented!(), NotImplementedError, and empty function bodies.\n\
         3. NEVER change existing function signatures, return types, struct fields, or public API.\n\
         4. NEVER remove existing code — only add or replace placeholder bodies.\n\
         5. Language: {lang_hint}"
    );

    let user = format!("Complete this file:\n\n{code}");

    (system, user)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_prompt() {
        let code = "fn main() { todo!(); }";
        let (system, user) = build_prompt(Language::Rust, code);
        assert!(system.contains("Rust"));
        assert!(system.contains("NEVER change existing function signatures"));
        assert!(user.contains(code));
    }
}
