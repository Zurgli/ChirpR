use std::collections::BTreeMap;

use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StyleGuide {
    pub sentence_case: bool,
    pub uppercase: bool,
    pub lowercase: bool,
    pub prepend: String,
    pub append: String,
}

impl StyleGuide {
    pub fn from_prompt(prompt: &str) -> Self {
        let mut guide = Self::default();

        for raw_line in prompt.lines() {
            let line = raw_line.trim();
            let lower = line.to_ascii_lowercase();
            if line.is_empty() {
                continue;
            }

            match lower.as_str() {
                "sentence case" | "sentence-case" | "capitalize sentences" => {
                    guide.sentence_case = true;
                    guide.uppercase = false;
                    guide.lowercase = false;
                }
                "uppercase" | "upper" => {
                    guide.uppercase = true;
                    guide.sentence_case = false;
                    guide.lowercase = false;
                }
                "lowercase" | "lower" => {
                    guide.lowercase = true;
                    guide.uppercase = false;
                    guide.sentence_case = false;
                }
                _ if lower.starts_with("prepend:") => {
                    guide.prepend = line
                        .split_once(':')
                        .map(|(_, value)| value.trim().to_string())
                        .unwrap_or_default();
                }
                _ if lower.starts_with("append:") => {
                    guide.append = line
                        .split_once(':')
                        .map(|(_, value)| value.trim().to_string())
                        .unwrap_or_default();
                }
                _ => {}
            }
        }

        guide
    }

    pub fn apply(&self, text: &str) -> String {
        let mut result = if self.uppercase {
            text.to_ascii_uppercase()
        } else if self.lowercase {
            text.to_ascii_lowercase()
        } else if self.sentence_case {
            sentence_case(text)
        } else {
            text.to_string()
        };

        if !self.prepend.is_empty() {
            result = format!("{} {}", self.prepend, result).trim().to_string();
        }
        if !self.append.is_empty() {
            result = format!("{result} {}", self.append).trim().to_string();
        }

        result
    }
}

#[derive(Debug, Clone)]
pub struct TextProcessor {
    word_overrides: BTreeMap<String, String>,
    override_pattern: Option<Regex>,
    style: StyleGuide,
}

impl TextProcessor {
    pub fn new(word_overrides: BTreeMap<String, String>, post_processing: &str) -> Self {
        let normalized_overrides = word_overrides
            .into_iter()
            .map(|(key, value)| (key.to_ascii_lowercase(), value))
            .collect::<BTreeMap<_, _>>();
        let override_pattern = build_override_pattern(&normalized_overrides);
        let style = StyleGuide::from_prompt(post_processing);

        Self {
            word_overrides: normalized_overrides,
            override_pattern,
            style,
        }
    }

    pub fn process(&self, text: &str) -> String {
        // Sanitize input before applying overrides so control characters cannot
        // leak into the injection path.
        let sanitized = sanitize(text, true);
        if sanitized.is_empty() {
            return sanitized;
        }

        let overridden = self.apply_word_overrides(&sanitized);
        let normalized = normalize_punctuation(&overridden);
        let styled = self.style.apply(&normalized);
        // Final sanitization prevents overrides or style directives from
        // reintroducing unsafe characters after processing.
        sanitize(&styled, false)
    }

    fn apply_word_overrides(&self, text: &str) -> String {
        let Some(pattern) = &self.override_pattern else {
            return text.to_string();
        };

        pattern
            .replace_all(text, |captures: &regex::Captures<'_>| {
                let matched = captures.get(0).map(|value| value.as_str()).unwrap_or("");
                self.word_overrides
                    .get(&matched.to_ascii_lowercase())
                    .cloned()
                    .unwrap_or_else(|| matched.to_string())
            })
            .to_string()
    }
}

pub fn sanitize(text: &str, strip_text: bool) -> String {
    let sanitized = text
        .chars()
        .filter(|ch| ch.is_ascii_graphic() || matches!(ch, ' ' | '\t' | '\n'))
        .collect::<String>();

    if strip_text {
        sanitized.trim().to_string()
    } else {
        sanitized
    }
}

pub fn normalize_punctuation(text: &str) -> String {
    let whitespace = Regex::new(r"\s+").expect("valid regex");
    let spacing_before_punctuation = Regex::new(r"\s+([,.;!?])").expect("valid regex");

    let collapsed = whitespace.replace_all(text, " ");
    spacing_before_punctuation
        .replace_all(&collapsed, "$1")
        .trim()
        .to_string()
}

fn build_override_pattern(overrides: &BTreeMap<String, String>) -> Option<Regex> {
    if overrides.is_empty() {
        return None;
    }

    let mut escaped = overrides
        .keys()
        .map(|word| regex::escape(word))
        .collect::<Vec<_>>();
    escaped.sort_by_key(|word| std::cmp::Reverse(word.len()));

    let pattern = format!(r"\b({})\b", escaped.join("|"));
    Regex::new(&format!("(?i){pattern}")).ok()
}

fn sentence_case(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut capitalize_next = true;

    for ch in text.chars() {
        if capitalize_next && ch.is_ascii_alphabetic() {
            result.push(ch.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch.to_ascii_lowercase());
        }

        if matches!(ch, '.' | '!' | '?' | '\n') {
            capitalize_next = true;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_removes_control_characters() {
        let processed = sanitize("Hello\x07 \x1bWorld\x08!", true);
        assert_eq!(processed, "Hello World!");
    }

    #[test]
    fn style_guide_supports_sentence_case_and_append() {
        let guide = StyleGuide::from_prompt("sentence case\nappend: done");
        assert_eq!(guide.apply("HELLO WORLD"), "Hello world done");
    }

    #[test]
    fn overrides_are_case_insensitive() {
        let processor = TextProcessor::new(
            BTreeMap::from([("parra keat".into(), "parakeet".into())]),
            "",
        );
        assert_eq!(processor.process("Parra Keat is here"), "parakeet is here");
    }

    #[test]
    fn overrides_cannot_reintroduce_control_characters() {
        let processor = TextProcessor::new(
            BTreeMap::from([("unsafe".into(), "back\x08space".into())]),
            "",
        );
        assert_eq!(processor.process("unsafe"), "backspace");
    }

    #[test]
    fn normalize_punctuation_collapses_spacing() {
        assert_eq!(
            normalize_punctuation("hello   ,   world !"),
            "hello, world!"
        );
    }
}
