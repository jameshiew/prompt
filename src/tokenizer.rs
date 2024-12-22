use text_splitter::{ChunkConfig, TextSplitter};
use tiktoken_rs::o200k_base_singleton;

const CHUNK_CAPACITY: usize = 2_000_000;

pub fn tokenize(text: &str) -> Vec<u32> {
    let bpe = o200k_base_singleton();
    let bpe = bpe.lock();

    let splitter = TextSplitter::new(ChunkConfig::new(CHUNK_CAPACITY));
    let chunks = splitter.chunks(text);

    let tokens: Vec<_> = chunks
        .into_iter()
        .flat_map(|chunk| bpe.encode_with_special_tokens(chunk))
        .collect();

    tokens
}
