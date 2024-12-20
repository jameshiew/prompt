use std::ops::Deref;

use text_splitter::{ChunkConfig, TextSplitter};
use tiktoken_rs::o200k_base_singleton;

const CHUNK_CAPACITY: usize = 100_000;

pub fn tokenize(text: &str) -> Vec<u32> {
    let bpe = o200k_base_singleton();
    let bpe = bpe.lock();

    let splitter = TextSplitter::new(ChunkConfig::new(CHUNK_CAPACITY).with_sizer(bpe.deref()));
    let chunks = splitter.chunks(text);
    let mut tokens = vec![];
    for chunk in chunks {
        tokens.append(&mut bpe.encode_with_special_tokens(chunk));
    }
    tokens
}
