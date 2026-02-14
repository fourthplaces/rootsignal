/// Truncate a string to at most `max_bytes` bytes at a character boundary.
pub fn truncate_to_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while !s.is_char_boundary(end) && end > 0 {
        end -= 1;
    }
    &s[..end]
}

/// Strip markdown code blocks from a response.
pub fn strip_code_blocks(response: &str) -> &str {
    response
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_to_char_boundary() {
        let text = "Hello 世界";
        let truncated = truncate_to_char_boundary(text, 8);
        assert!(truncated.len() <= 8);
        assert!(text.starts_with(truncated));
    }

    #[test]
    fn test_truncate_within_bounds() {
        let text = "Hello";
        assert_eq!(truncate_to_char_boundary(text, 100), "Hello");
    }

    #[test]
    fn test_strip_code_blocks() {
        assert_eq!(strip_code_blocks("```json\n{}\n```"), "{}");
        assert_eq!(strip_code_blocks("```\n{}\n```"), "{}");
        assert_eq!(strip_code_blocks("{}"), "{}");
    }
}
