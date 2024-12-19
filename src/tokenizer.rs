use tiktoken_rs::o200k_base_singleton;

pub fn tokenize(text: &str) -> Vec<u32> {
    let bpe = o200k_base_singleton();
    let bpe = bpe.lock();
    bpe.encode_with_special_tokens(text)
}
