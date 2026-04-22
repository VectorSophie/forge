use crate::language::Language;

pub fn build_prompt(language: Language, code: &str) -> String {
    let mut prompt = String::new();
    prompt.push_str("You are an expert AI coding assistant. Your task is to complete the following code file by replacing any TODO, pass, NotImplementedError, or incomplete logic with robust, idiomatic code.\n");
    prompt.push_str("Output ONLY the completed code. Do NOT wrap it in markdown blocks (e.g. ```). Do NOT provide explanations. Add imports only if needed.\n\n");

    match language {
        Language::Python => {
            prompt.push_str("Language: Python. Follow PEP8. Ensure proper asyncio/argparse usage if present in the skeleton.\n\n");
        }
        Language::Rust => {
            prompt.push_str(
                "Language: Rust. Use idiomatic Result, ownership, match, and error handling.\n\n",
            );
        }
        Language::JavaScript => {
            prompt.push_str("Language: JavaScript. Use ES6+ syntax, async/promises, and minimal dependencies.\n\n");
        }
        Language::TypeScript => {
            prompt.push_str(
                "Language: TypeScript. Use strong typing, ES6+ syntax, and idiomatic patterns.\n\n",
            );
        }
        Language::Dockerfile => {
            prompt.push_str(
                "Language: Dockerfile. Preserve sensible layering and minimal image size.\n\n",
            );
        }
        Language::Unknown => {
            prompt.push_str("Language: Unknown generic text code file.\n\n");
        }
    }

    prompt.push_str("Original Code:\n");
    prompt.push_str(code);
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_prompt() {
        let code = "fn main() { TODO!(); }";
        let prompt = build_prompt(Language::Rust, code);
        assert!(prompt.contains("Language: Rust. Use idiomatic Result"));
        assert!(prompt.contains(code));
    }
}
