use tiktoken_rs::o200k_base_singleton;

pub fn tokenize(text: &str) -> Vec<u32> {
    o200k_base_singleton().encode_with_special_tokens(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PREVIOUS_CHUNK_CAPACITY: usize = 2_000_000;

    fn assert_direct_parity(text: &str) {
        let expected = o200k_base_singleton().encode_with_special_tokens(text);

        assert_eq!(tokenize(text), expected);
    }

    #[test]
    fn test_tokenize_empty() {
        assert_direct_parity("");
    }

    #[test]
    fn test_tokenize_preserves_whitespace() {
        for text in [" hello", "hello ", "1 hello\n", "\n\nhello\t"] {
            assert_direct_parity(text);
        }
    }

    #[test]
    fn test_tokenize_above_previous_chunk_capacity() {
        let text = "a ".repeat(PREVIOUS_CHUNK_CAPACITY / 2 + 1);

        assert_direct_parity(&text);
    }

    #[test]
    fn test_tokenize_preserves_boundary_merges() {
        let text = format!("{}bc", "a".repeat(PREVIOUS_CHUNK_CAPACITY - 1));

        assert_direct_parity(&text);
    }
}
