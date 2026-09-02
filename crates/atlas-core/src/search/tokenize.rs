const STOP: &[&str] = &["the","a","an","and","or","of","in","on","to","is","are","was","were","be","it","this","that","for","with","as","at","by","from","which"];

pub fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '.' || c == '-' || c == '_'))
        .map(|t| t.trim_matches(|c: char| c == '.' || c == '-' || c == '_'))
        .filter(|t| t.len() > 1 && !STOP.contains(t))
        .map(|t| t.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::tokenize;
    #[test]
    fn lowercases_splits_and_drops_stopwords() {
        assert_eq!(tokenize("The Tailwind config lives in CSS!"), vec!["tailwind", "config", "lives", "css"]);
    }
    #[test]
    fn keeps_identifiers_with_dots_and_dashes() {
        assert_eq!(tokenize("run bun.lock and react-doctor"), vec!["run", "bun.lock", "react-doctor"]);
    }
}
