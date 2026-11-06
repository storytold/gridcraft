//! Property tests for [`decompress`] and the test-only [`compress`] (design properties P1 and
//! P2), driven by a seeded xorshift64 generator so every failure replays.
//!
//! - P1 (round trip): `decompress(compress(x))` is `x` followed only by 0x00 padding, which only
//!   a raw last chunk adds (§2.4.1.3.10), and is at most `len(x)` rounded up to 4,096 bytes. With
//!   a compressed last chunk, or a length that is a multiple of 4,096, it is exactly `x`. Inputs
//!   are 0..=65,536 bytes long, biased toward chunk boundaries, mixing runs, short repeated
//!   patterns, text-like bytes, uniform random bytes, echoes of earlier bytes, and incompressible
//!   tails of 3,641..=4,095 bytes (the only inputs `compress` pads).
//! - P2 (no panic, bounded output): `decompress(arbitrary, max_out)` is `Ok(v)` with
//!   `v.len() <= max_out`, or `Err`, and never panics. Inputs are uniform random bytes,
//!   containers with valid chunk signatures and random tokens, damaged `compress` output, and
//!   valid containers built to overrun a small `max_out`. Each input runs with
//!   [`MAX_DECOMPRESSED`] and with a small limit.
//!
//! Knobs (environment variables):
//! - `GRIDCRAFT_FUZZ_SEEDS` (default 64): P1 runs this many seeds, P2 32 inputs per seed (2,048
//!   by default). Soak with
//!   `GRIDCRAFT_FUZZ_SEEDS=500 cargo test --release -p gridcraft-vba --lib ovba::props::`.
//! - `GRIDCRAFT_FUZZ_SEED=17`: replay that one seed (failure messages print the command line).
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::{COMPRESSED_FLAG, DECOMPRESSED_CHUNK_MAX, MAX_DECOMPRESSED, OvbaError, SIGNATURE_BITS, SIGNATURE_BYTE, SIZE_MASK, compress, decompress};

/// Default `GRIDCRAFT_FUZZ_SEEDS`: small, so `cargo xtask ci` stays fast.
const DEFAULT_SEEDS: u64 = 64;
/// P2 inputs per seed.
const P2_INPUTS_PER_SEED: u64 = 32;
/// The longest P1 input.
const MAX_P1_LEN: usize = 65_536;
/// Lengths around the 4,096-byte chunk size, picked often.
const BOUNDARY_LENGTHS: [usize; 11] = [0, 1, 2, 3, 4095, 4096, 4097, 8191, 8192, 8193, 65_536];
/// Random tails this long can't fit as literals plus FlagBytes, so `compress` stores them raw and
/// pads them; 3,640 bytes still fit (see `short_raw_chunk_is_padded`).
const PADDED_TAIL: (usize, usize) = (3641, 4095);
/// A valid CompressedChunk that decompresses to 4,096 bytes: 'a', then a copy of offset 1 and
/// length 4,095.
const FULL_CHUNK: [u8; 6] = [0x03, 0xB0, 0x02, 0x61, 0xFC, 0x0F];
/// The §3.2.3 example chunk: 73 bytes.
const SMALL_CHUNK: [u8; 6] = [0x03, 0xB0, 0x02, 0x61, 0x45, 0x00];
/// Distinct streams so P1 and P2 seeds don't share random sequences.
const P1_STREAM: u64 = 0x5031_5031_5031_5031;
const P2_STREAM: u64 = 0x5032_5032_5032_5032;

// ---------------------------------------------------------------- deterministic randomness

/// xorshift64: tiny and deterministic.
struct Rng(u64);

impl Rng {
    /// A generator for `seed` on its own `stream`, so each property draws an independent sequence from the same seed list. The state is mixed and never zero, and the first outputs are discarded.
    fn new(seed: u64, stream: u64) -> Self {
        // Spread consecutive seeds over the state space; xorshift needs a nonzero state.
        let mixed = seed.wrapping_add(1).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ stream;
        let mut rng = Self(if mixed == 0 { 0x2545_F491_4F6C_DD1D } else { mixed });
        for _ in 0..4 {
            rng.next();
        }
        rng
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn byte(&mut self) -> u8 {
        (self.next() >> 56) as u8
    }

    /// A value in `0..n`, or 0 when `n` is 0.
    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }

    /// Inclusive range.
    fn range(&mut self, lo: usize, hi: usize) -> usize {
        lo + self.below(hi.saturating_sub(lo) + 1)
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }

    fn fill(&mut self, out: &mut Vec<u8>, n: usize) {
        out.extend((0..n).map(|_| self.byte()));
    }
}

fn env_num(name: &str) -> Option<u64> {
    std::env::var(name).ok().and_then(|v| v.trim().parse().ok())
}

/// The seeds to run: `GRIDCRAFT_FUZZ_SEED` alone, or `0..count`.
fn seed_list(per_seed: u64) -> (Vec<u64>, bool) {
    match env_num("GRIDCRAFT_FUZZ_SEED") {
        Some(one) => (vec![one], true),
        None => {
            let seeds = env_num("GRIDCRAFT_FUZZ_SEEDS").unwrap_or(DEFAULT_SEEDS);
            ((0..seeds.saturating_mul(per_seed)).collect(), false)
        }
    }
}

fn replay(seed: u64, test: &str) -> String {
    format!("replay: GRIDCRAFT_FUZZ_SEED={seed} cargo test -p gridcraft-vba --lib ovba::props::{test}")
}

// ---------------------------------------------------------------- P1: round trip

/// Appends one segment of at least one byte, without taking `data` past `end` bytes.
fn push_segment(rng: &mut Rng, data: &mut Vec<u8>, end: usize) {
    const WORDS: [&[u8]; 14] = [
        b"Sub ",
        b"End Sub",
        b"Dim ",
        b" As Long",
        b"Range(\"A1\")",
        b".Value",
        b" = ",
        b"If ",
        b" Then",
        b"MsgBox ",
        b"\r\n",
        b"    ",
        b"' note",
        b"x",
    ];
    let n = rng.range(1, end.saturating_sub(data.len()).min(6000));
    match rng.below(5) {
        // A long run of one byte.
        0 => {
            let byte = rng.byte();
            data.resize(data.len() + n, byte);
        }
        // A short pattern, repeated.
        1 => {
            let pattern: Vec<u8> = (0..rng.range(1, 16)).map(|_| rng.byte()).collect();
            data.extend(pattern.iter().cycle().take(n));
        }
        // Text-like: words, numbers and line breaks.
        2 => {
            let mut text = Vec::with_capacity(n + 16);
            while text.len() < n {
                if rng.chance(20) {
                    text.extend_from_slice(rng.below(100_000).to_string().as_bytes());
                } else {
                    text.extend_from_slice(WORDS[rng.below(WORDS.len())]);
                }
            }
            text.truncate(n);
            data.extend(text);
        }
        // Uniform random bytes.
        3 => rng.fill(data, n),
        // An echo of earlier bytes, so matches come from every distance.
        _ => {
            if data.is_empty() {
                rng.fill(data, n);
            } else {
                let start = rng.below(data.len());
                let piece: Vec<u8> = data[start..].iter().copied().take(n).collect();
                data.extend(piece);
            }
        }
    }
}

/// P1's input for `seed`: a random length (often a chunk-boundary length) filled with compressible segments; every eighth seed instead ends in an incompressible tail after whole chunks, so the last chunk can be raw.
fn p1_input(seed: u64) -> Vec<u8> {
    let mut rng = Rng::new(seed, P1_STREAM);
    // Every eighth seed ends in an incompressible tail after whole chunks.
    let tail = if seed % 8 == 7 { rng.range(PADDED_TAIL.0, PADDED_TAIL.1) } else { 0 };
    let len = if tail > 0 {
        rng.below(MAX_P1_LEN / DECOMPRESSED_CHUNK_MAX) * DECOMPRESSED_CHUNK_MAX + tail
    } else if rng.chance(40) {
        BOUNDARY_LENGTHS[rng.below(BOUNDARY_LENGTHS.len())]
    } else {
        rng.range(0, MAX_P1_LEN)
    };
    let body = len - tail;
    let mut data = Vec::with_capacity(len);
    while data.len() < body {
        push_segment(&mut rng, &mut data, body);
    }
    rng.fill(&mut data, tail);
    data
}

/// Whether the container's last chunk is raw (CompressedChunkFlag 0), walking the chunk headers;
/// `false` for a container without chunks.
fn last_chunk_is_raw(container: &[u8]) -> bool {
    let mut at = 1;
    let mut raw = false;
    while let Some(&[lo, hi]) = container.get(at..at + 2) {
        let header = u16::from_le_bytes([lo, hi]);
        raw = header & COMPRESSED_FLAG == 0;
        at += usize::from(header & SIZE_MASK) + 3;
    }
    raw
}

/// Checks P1 for `data`. Returns whether the output was padded.
fn check_round_trip(data: &[u8]) -> Result<bool, String> {
    let compressed = compress(data);
    let out = decompress(&compressed, MAX_DECOMPRESSED).map_err(|e| format!("decompress(compress(x)) failed: {e:?}"))?;
    let bound = data.len().div_ceil(DECOMPRESSED_CHUNK_MAX) * DECOMPRESSED_CHUNK_MAX;
    if let Some(i) = data.iter().zip(&out).position(|(a, b)| a != b) {
        return Err(format!("output differs from the input at byte {i}"));
    }
    if out.len() < data.len() {
        return Err(format!("output is {} bytes, shorter than the input", out.len()));
    }
    if let Some(i) = out[data.len()..].iter().position(|&b| b != 0) {
        return Err(format!("padding byte {} (output byte {}) is not 0x00", i, data.len() + i));
    }
    if out.len() > bound {
        return Err(format!("output is {} bytes, more than the input rounded up to 4096 ({bound})", out.len()));
    }
    let raw_last = last_chunk_is_raw(&compressed);
    if (!raw_last || data.len().is_multiple_of(DECOMPRESSED_CHUNK_MAX)) && out.len() != data.len() {
        return Err(format!("output is {} bytes but no raw short last chunk explains padding (last chunk raw: {raw_last})", out.len()));
    }
    if raw_last && out.len() != bound {
        return Err(format!("a raw last chunk should decompress to a full chunk ({bound} bytes), got {}", out.len()));
    }
    Ok(out.len() > data.len())
}

/// P1: `decompress(compress(x))` is `x` plus only the padding a raw last chunk adds.
///
/// **Validates: Requirements 4.9**
#[test]
fn p1_round_trip() {
    let (seeds, replaying) = seed_list(1);
    let mut padded = 0;
    for &seed in &seeds {
        let data = p1_input(seed);
        match check_round_trip(&data) {
            Ok(was_padded) => padded += usize::from(was_padded),
            Err(why) => panic!("P1 failed for seed {seed} ({} bytes): {why}\n  {}", data.len(), replay(seed, "p1_round_trip")),
        }
    }
    // The generator must reach the padded case; every eighth seed aims for it.
    if !replaying && seeds.len() >= 16 {
        assert!(padded > 0, "P1 never produced a padded raw last chunk in {} seeds", seeds.len());
    }
}

// ---------------------------------------------------------------- P2: arbitrary input

/// Up to 300 random bytes, half the time starting with the container signature byte, so `decompress` gets past the signature check.
fn uniform_input(rng: &mut Rng) -> Vec<u8> {
    let mut input = Vec::new();
    let len = rng.range(0, 300);
    rng.fill(&mut input, len);
    if rng.chance(50)
        && let Some(first) = input.first_mut()
    {
        *first = SIGNATURE_BYTE;
    }
    input
}

/// A container whose chunk headers carry the valid 0b011 signature, with random flags, sizes
/// that usually match the body, and random literals and copy tokens.
fn structured_input(rng: &mut Rng) -> Vec<u8> {
    let mut out = vec![SIGNATURE_BYTE];
    for _ in 0..rng.range(1, 4) {
        let compressed = rng.chance(80);
        let body_len = if compressed { rng.range(1, 400) } else { DECOMPRESSED_CHUNK_MAX };
        // CompressedChunkSize is the chunk length (header + body) minus 3.
        let size_field = if rng.chance(75) { body_len - 1 } else { rng.below(4096) };
        let mut header = SIGNATURE_BITS | (size_field as u16 & SIZE_MASK);
        if compressed {
            header |= COMPRESSED_FLAG;
        }
        out.extend_from_slice(&header.to_le_bytes());
        let start = out.len();
        if compressed {
            while out.len() - start < body_len {
                let flags = rng.byte();
                out.push(flags);
                for index in 0..8 {
                    if (flags >> index) & 1 == 0 {
                        out.push(rng.byte());
                    } else if rng.chance(50) {
                        out.extend_from_slice(&(rng.next() as u16).to_le_bytes());
                    } else {
                        // A short reach and length, which is often valid.
                        let token = ((rng.below(4) << 12) | rng.below(64)) as u16;
                        out.extend_from_slice(&token.to_le_bytes());
                    }
                }
            }
            out.truncate(start + body_len);
        } else {
            rng.fill(&mut out, body_len);
        }
    }
    if rng.chance(20) {
        let cut = rng.range(1, out.len());
        out.truncate(cut);
    }
    out
}

/// Valid `compress` output with a few bytes flipped, inserted or cut.
fn damaged_input(rng: &mut Rng) -> Vec<u8> {
    let len = rng.range(0, 9000);
    let mut data = Vec::with_capacity(len);
    while data.len() < len {
        push_segment(rng, &mut data, len);
    }
    let mut out = compress(&data);
    for _ in 0..rng.range(1, 4) {
        // Leave the signature byte alone so the decoder gets past it.
        let at = rng.range(1, out.len().saturating_sub(1).max(1));
        match rng.below(3) {
            0 => {
                if let Some(byte) = out.get_mut(at) {
                    *byte ^= rng.byte() | 1;
                }
            }
            1 => out.insert(at.min(out.len()), rng.byte()),
            _ => out.truncate(at),
        }
    }
    out
}

/// A valid container of full (4,096-byte) and small (73-byte) chunks, and a `max_out` near its
/// decompressed size. Returns the input, the limit and the decompressed size.
fn overrun_input(rng: &mut Rng) -> (Vec<u8>, usize, usize) {
    let mut out = vec![SIGNATURE_BYTE];
    let mut total = 0;
    for _ in 0..rng.range(1, 8) {
        if rng.chance(70) {
            out.extend_from_slice(&FULL_CHUNK);
            total += DECOMPRESSED_CHUNK_MAX;
        } else {
            out.extend_from_slice(&SMALL_CHUNK);
            total += 73;
        }
    }
    let max_out = if rng.chance(70) { rng.range(total.saturating_sub(100), total + 100) } else { rng.below(total + 1) };
    (out, max_out, total)
}

/// Runs `decompress`, turning a panic into an error message.
fn run(input: &[u8], max_out: usize) -> Result<Outcome, String> {
    catch_unwind(AssertUnwindSafe(|| decompress(input, max_out))).map_err(|_| format!("decompress panicked (max_out {max_out})"))
}

/// Runs `decompress` on `input` and fails if it panics or returns more than `max_out` bytes.
fn check_bounded(input: &[u8], max_out: usize) -> Result<Outcome, String> {
    let result = run(input, max_out)?;
    if let Ok(v) = &result
        && v.len() > max_out
    {
        return Err(format!("decompress returned {} bytes, more than max_out {max_out}", v.len()));
    }
    Ok(result)
}

type Outcome = Result<Vec<u8>, OvbaError>;

/// Checks P2 for one seed. Returns the results under [`MAX_DECOMPRESSED`] and the small limit.
fn check_arbitrary(seed: u64) -> Result<[Outcome; 2], String> {
    let mut rng = Rng::new(seed, P2_STREAM);
    let (input, small_max, expected_total) = match seed % 4 {
        0 => (uniform_input(&mut rng), rng.range(0, 9000), None),
        1 => (structured_input(&mut rng), rng.range(0, 9000), None),
        2 => (damaged_input(&mut rng), rng.range(0, 9000), None),
        _ => {
            let (input, max_out, total) = overrun_input(&mut rng);
            (input, max_out, Some(total))
        }
    };
    let full = check_bounded(&input, MAX_DECOMPRESSED)?;
    let limited = check_bounded(&input, small_max)?;
    // The limit only adds OutputLimit: output that fits comes back unchanged, and output that
    // doesn't fit is refused.
    if let Ok(v) = &full {
        let expected = if v.len() <= small_max { Ok(v.clone()) } else { Err(OvbaError::OutputLimit) };
        if limited != expected {
            return Err(format!("with max_out {small_max}, got {limited:?} for {} bytes of output", v.len()));
        }
    }
    if let Some(total) = expected_total {
        match &full {
            Ok(v) if v.len() == total => {}
            other => return Err(format!("a valid {total}-byte container decompressed to {other:?}")),
        }
    }
    Ok([full, limited])
}

/// P2: arbitrary input gives `Ok` within the limit or a typed error, never a panic.
///
/// **Validates: Requirements 4.10**
#[test]
fn p2_arbitrary_input_never_panics() {
    let (seeds, replaying) = seed_list(P2_INPUTS_PER_SEED);
    // How often each outcome came up: Ok, then each error.
    let mut seen = [0usize; 5];
    for &seed in &seeds {
        match check_arbitrary(seed) {
            Ok(results) => {
                for result in results {
                    let slot = match result {
                        Ok(_) => 0,
                        Err(OvbaError::InvalidSignature) => 1,
                        Err(OvbaError::InvalidChunk { .. }) => 2,
                        Err(OvbaError::InvalidCopyToken { .. }) => 3,
                        Err(OvbaError::OutputLimit) => 4,
                    };
                    seen[slot] += 1;
                }
            }
            Err(why) => panic!("P2 failed for seed {seed}: {why}\n  {}", replay(seed, "p2_arbitrary_input_never_panics")),
        }
    }
    // The generators must get past the signature check and reach every outcome.
    if !replaying && seeds.len() >= 256 {
        assert!(seen.iter().all(|&n| n > 0), "P2 outcomes [ok, signature, chunk, copy token, limit] = {seen:?} in {} inputs", seeds.len());
    }
}

/// P2's output cap at its real size: one byte past 16 MiB is refused, 16 MiB exactly is not.
///
/// **Validates: Requirements 4.10**
#[test]
fn p2_output_never_passes_16_mib() {
    let chunks = MAX_DECOMPRESSED / DECOMPRESSED_CHUNK_MAX;
    let mut input = vec![SIGNATURE_BYTE];
    for _ in 0..chunks {
        input.extend_from_slice(&FULL_CHUNK);
    }
    assert_eq!(decompress(&input, MAX_DECOMPRESSED).map(|v| v.len()), Ok(MAX_DECOMPRESSED));
    input.extend_from_slice(&[0x01, 0xB0, 0x00, 0x61]); // a chunk with one literal
    assert_eq!(decompress(&input, MAX_DECOMPRESSED), Err(OvbaError::OutputLimit));
}
