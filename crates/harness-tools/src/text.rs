//! UTF-8-safe limits for tool output and user-facing previews.

/// Return at most `max_chars` Unicode scalar values without allocating.
pub fn char_prefix(text: &str, max_chars: usize) -> &str {
    let end = text
        .char_indices()
        .nth(max_chars)
        .map_or(text.len(), |(i, _)| i);
    &text[..end]
}

/// Return the longest valid UTF-8 prefix within a byte budget.
pub fn byte_prefix(text: &str, max_bytes: usize) -> &str {
    let mut end = max_bytes.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Return the longest valid UTF-8 suffix within a byte budget.
pub fn byte_suffix(text: &str, max_bytes: usize) -> &str {
    let mut start = text.len().saturating_sub(max_bytes);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn character_limits_preserve_multilingual_text() {
        assert_eq!(char_prefix("aé中🦀z", 4), "aé中🦀");
        assert_eq!(char_prefix("🦀", 0), "");
        assert_eq!(char_prefix("é", 100), "é");
        assert_eq!(char_prefix("", 0), "");
    }

    #[test]
    fn byte_limits_never_split_unicode_and_respect_budgets() {
        let text = "aé中🦀z";
        for budget in 0..=text.len() + 1 {
            let head = byte_prefix(text, budget);
            let tail = byte_suffix(text, budget);
            assert!(text.starts_with(head) && text.ends_with(tail));
            assert!(head.len() <= budget && tail.len() <= budget);
        }
        assert_eq!(byte_prefix(text, 2), "a");
        assert_eq!(byte_suffix(text, 4), "z");
    }
}
