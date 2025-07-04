use text_splitter::{ChunkConfig, TextSplitter};
use tiktoken_rs::o200k_base_singleton;

const CHUNK_CAPACITY: usize = 2_000_000;

pub fn tokenize(text: &str) -> Vec<u32> {
    if text.is_empty() {
        return vec![];
    }

    let bpe = o200k_base_singleton();

    let splitter = TextSplitter::new(ChunkConfig::new(CHUNK_CAPACITY));
    let chunks = splitter.chunks(text);

    let tokens: Vec<_> = chunks
        .into_iter()
        .flat_map(|chunk| bpe.encode_with_special_tokens(chunk))
        .collect();

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_empty() {
        let text = "";

        let result = tokenize(text);

        assert!(result.is_empty());
    }
}
