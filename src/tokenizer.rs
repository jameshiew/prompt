use std::ops::Deref;

use text_splitter::{ChunkConfig, TextSplitter};
use tiktoken_rs::o200k_base_singleton;

pub fn tokenize(text: &str) -> Vec<u32> {
    let bpe = o200k_base_singleton();
    let bpe = bpe.lock();

    // tokenize chunks of text at a time in case the text is large
    // to avoid tokenizer stack overflow
    let max_tokens = 1_000;
    let splitter = TextSplitter::new(ChunkConfig::new(max_tokens).with_sizer(bpe.deref()));
    let chunks = splitter.chunks(text);
    let mut tokens = vec![];
    for chunk in chunks {
        tokens.append(&mut bpe.encode_with_special_tokens(chunk));
    }
    tokens
}
