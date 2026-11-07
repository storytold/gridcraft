//! Marian tokenizer from a sentencepiece model and `vocab.json`, as shipped by
//! `Helsinki-NLP/opus-mt-*` and its ONNX conversions.
//!
//! Adapted from `candle-examples/examples/marian-mt/spm.rs` in Hugging Face's candle
//! (https://github.com/huggingface/candle), Copyright the candle authors, licensed MIT OR
//! Apache-2.0. The algorithm (a minimal protobuf reader for the sentencepiece model, a Unigram
//! model built in `vocab.json` id order, the Metaspace pre-tokenizer/decoder and the precompiled
//! normalizer) is unchanged. `anyhow` was replaced with this crate's `Result<_, String>` so the
//! backend has no extra dependency, and comments were rewritten for this repository.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::Path;

use tokenizers::decoders::metaspace::Metaspace as MetaspaceDecoder;
use tokenizers::models::unigram::Unigram;
use tokenizers::normalizers::replace::ReplacePattern;
use tokenizers::normalizers::{Precompiled, Replace, Sequence};
use tokenizers::pre_tokenizers::metaspace::{Metaspace, PrependScheme};
use tokenizers::{AddedToken, Tokenizer};

type Result<T, E = String> = std::result::Result<T, E>;

/// Placeholder for ids that are in `vocab.json` but have no matching sentencepiece.
const UNUSED_PIECE: &str = "<NIL>";
const UNUSED_SCORE: f64 = -100.0;

/// Special Marian tokens.
const SPECIAL_TOKENS: [&str; 3] = ["</s>", "<unk>", "<pad>"];

/// Minimal protobuf reader for `ModelProto`.
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

enum Value<'a> {
    Varint(u64),
    Fixed64,
    Bytes(&'a [u8]),
    Fixed32(u32),
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn is_empty(&self) -> bool {
        self.pos >= self.buf.len()
    }

    fn varint(&mut self) -> Result<u64> {
        let mut value = 0u64;
        let mut shift = 0;
        loop {
            let Some(byte) = self.buf.get(self.pos).copied() else { return Err("protobuf: unexpected end of data".into()) };
            self.pos += 1;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
            shift += 7;
            if shift >= 64 {
                return Err("protobuf: varint is too large".into());
            }
        }
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(len).filter(|end| *end <= self.buf.len()).ok_or("protobuf: unexpected end of data")?;
        let bytes = &self.buf[self.pos..end];
        self.pos = end;
        Ok(bytes)
    }

    fn next_field(&mut self) -> Result<(u64, Value<'a>)> {
        let key = self.varint()?;
        let value = match key & 7 {
            0 => Value::Varint(self.varint()?),
            1 => {
                self.take(8)?;
                Value::Fixed64
            }
            2 => {
                let len = usize::try_from(self.varint()?).map_err(|_| "protobuf: field is too long")?;
                Value::Bytes(self.take(len)?)
            }
            5 => {
                let bytes = self.take(4)?;
                let array: [u8; 4] = bytes.try_into().map_err(|_| "protobuf: short fixed32")?;
                Value::Fixed32(u32::from_le_bytes(array))
            }
            other => return Err(format!("protobuf: unsupported wire type {other}")),
        };
        Ok((key >> 3, value))
    }
}

#[derive(Default)]
struct SpmModel {
    /// `ModelProto.pieces`, in sentencepiece order rather than `vocab.json` order.
    pieces: Vec<(String, f64)>,
    unk_id: i64,
    byte_fallback: bool,
    precompiled_charsmap: Vec<u8>,
}

impl SpmModel {
    fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let data = std::fs::read(path).map_err(|e| format!("cannot read the spm model {}: {e}", path.display()))?;
        Self::from_slice(&data).map_err(|e| format!("cannot parse the spm model {}: {e}", path.display()))
    }

    fn from_slice(data: &[u8]) -> Result<Self> {
        let mut model = Self::default();
        let mut reader = Reader::new(data);
        while !reader.is_empty() {
            match reader.next_field()? {
                // ModelProto.pieces
                (1, Value::Bytes(bytes)) => model.pieces.push(Self::piece(bytes)?),
                // ModelProto.trainer_spec
                (2, Value::Bytes(bytes)) => {
                    let mut inner = Reader::new(bytes);
                    while !inner.is_empty() {
                        match inner.next_field()? {
                            (35, Value::Varint(v)) => model.byte_fallback = v != 0,
                            (40, Value::Varint(v)) => model.unk_id = i64::try_from(v).unwrap_or(-1),
                            _ => {}
                        }
                    }
                }
                // ModelProto.normalizer_spec
                (3, Value::Bytes(bytes)) => {
                    let mut inner = Reader::new(bytes);
                    while !inner.is_empty() {
                        if let (2, Value::Bytes(bytes)) = inner.next_field()? {
                            model.precompiled_charsmap = bytes.to_vec();
                        }
                    }
                }
                _ => {}
            }
        }
        if model.pieces.is_empty() {
            return Err("no sentencepiece pieces found".into());
        }
        Ok(model)
    }

    /// `ModelProto.SentencePiece`: only the piece and its score are needed.
    fn piece(data: &[u8]) -> Result<(String, f64)> {
        let (mut piece, mut score) = (None, 0f64);
        let mut reader = Reader::new(data);
        while !reader.is_empty() {
            match reader.next_field()? {
                (1, Value::Bytes(bytes)) => piece = Some(String::from_utf8(bytes.to_vec()).map_err(|e| format!("a piece is not UTF-8: {e}"))?),
                (2, Value::Fixed32(v)) => score = f64::from(f32::from_bits(v)),
                _ => {}
            }
        }
        piece.map(|piece| (piece, score)).ok_or_else(|| "a sentencepiece piece has no `piece` field".into())
    }
}

/// Builds a tokenizer from a sentencepiece model and the `vocab.json` beside it. Marian shares
/// one vocabulary for both directions; only the spm model differs between encoder and decoder.
pub fn tokenizer<P: AsRef<Path>, Q: AsRef<Path>>(spm_file: P, vocab_file: Q) -> Result<Tokenizer> {
    let spm = SpmModel::from_file(spm_file)?;
    let vocab_file = vocab_file.as_ref();
    let vocab: HashMap<String, usize> = {
        let vocab = std::fs::read(vocab_file).map_err(|e| format!("cannot read the vocabulary {}: {e}", vocab_file.display()))?;
        serde_json::from_slice(&vocab).map_err(|e| format!("cannot parse the vocabulary {}: {e}", vocab_file.display()))?
    };
    let vocab_size = vocab.values().max().map(|max| max.saturating_add(1)).ok_or_else(|| format!("{} is empty", vocab_file.display()))?;
    let mut pieces = vec![(UNUSED_PIECE.to_string(), UNUSED_SCORE); vocab_size];
    for (piece, score) in spm.pieces {
        if let Some(&index) = vocab.get(&piece)
            && index < pieces.len()
        {
            pieces[index] = (piece, score);
        }
    }
    let special_tokens: Vec<(&str, usize)> = SPECIAL_TOKENS.iter().filter_map(|token| vocab.get(*token).map(|&index| (*token, index))).collect();
    for (token, index) in &special_tokens {
        if let Some(entry) = pieces.get_mut(*index)
            && entry.0 == UNUSED_PIECE
        {
            *entry = (token.to_string(), 0.0);
        }
    }
    let unk_id = match vocab.get("<unk>") {
        Some(&id) => id,
        None if spm.unk_id >= 0 && (spm.unk_id as usize) < vocab_size => spm.unk_id as usize,
        None => return Err(format!("no <unk> token in {}", vocab_file.display())),
    };
    let model = Unigram::from(pieces, Some(unk_id), spm.byte_fallback).map_err(|e| format!("cannot build the unigram model: {e}"))?;
    let mut tokenizer = Tokenizer::new(model);
    let precompiled = Precompiled::from(&spm.precompiled_charsmap).map_err(|e| format!("cannot build the normalizer: {e}"))?;
    let replace = Replace::new(ReplacePattern::Regex(r" {2,}".to_string()), " ").map_err(|e| format!("cannot build the normalizer: {e}"))?;
    tokenizer
        .with_normalizer(Some(Sequence::new(vec![precompiled.into(), replace.into()])))
        .map_err(|e| format!("cannot set the normalizer: {e}"))?;
    tokenizer.with_pre_tokenizer(Some(Metaspace::new('▁', PrependScheme::Always, true)));
    tokenizer.with_decoder(Some(MetaspaceDecoder::new('▁', PrependScheme::Always, true)));
    tokenizer
        .add_special_tokens(special_tokens.into_iter().map(|(token, _)| AddedToken::from(token, true)))
        .map_err(|e| format!("cannot add the special tokens: {e}"))?;
    Ok(tokenizer)
}
