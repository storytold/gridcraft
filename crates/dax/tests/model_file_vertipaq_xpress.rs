//! Xpress8 encoder/decoder properties required by the VertiPaq specification (§4.4, §4.5).

use gridcraft_dax::model_file::xpress8;

fn corpus() -> Vec<Vec<u8>> {
    let mut cases: Vec<Vec<u8>> = Vec::new();
    for length in [0usize, 1, 2, 3, 7, 8, 9, 31, 32, 33, 63, 64, 65, 255, 256, 4095, 4096, 4097, 8191, 8192, 8193, 65535, 65536] {
        let mut state = 0x9E37_79B9u32 ^ (length as u32);
        cases.push(
            (0..length)
                .map(|_| {
                    state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (state >> 24) as u8
                })
                .collect(),
        );
        cases.push((0..length).map(|index| (index % 256) as u8).collect());
        cases.push(vec![0x5A; length]);
        cases.push((0..length).map(|index| (index % 13) as u8).collect());
    }
    for value in 0..=255u8 {
        cases.push(vec![value; 333]);
        cases.push((0..1000).map(|index| if index % 2 == 0 { value } else { 255 - value }).collect());
    }
    cases
}

#[test]
fn round_trip_corpus() {
    for case in corpus() {
        // Declining is legal for incompressible input; the specification only forbids an
        // output that is equal to or larger than the input.
        if let Some(encoded) = xpress8::compress(&case) {
            assert!(encoded.len() >= 8, "encoder emitted a stream shorter than the chunk minimum");
            assert!(encoded.len() < case.len(), "encoder emitted a stream that is not smaller");
            let decoded = xpress8::decompress(&encoded, case.len()).expect("decode");
            assert_eq!(decoded, case);
        }
    }
    assert!(xpress8::compress(&vec![0u8; 1000]).is_some());
    assert!(xpress8::compress(&(0..1000).map(|index| (index % 5) as u8).collect::<Vec<_>>()).is_some());
}

#[test]
fn encoder_is_deterministic() {
    for case in corpus().into_iter().take(64) {
        assert_eq!(xpress8::compress(&case), xpress8::compress(&case));
    }
}

#[test]
fn truncation_never_panics_and_errors() {
    let input: Vec<u8> = (0..9000).map(|index| (index % 251) as u8).collect();
    let encoded = xpress8::compress(&input).expect("compressible");
    for end in 0..encoded.len() {
        let result = xpress8::decompress(&encoded[..end], input.len());
        assert!(result.is_err(), "prefix of length {end} decoded successfully");
    }
    // Every single-byte flip must either error or decode to the full declared size; the only
    // requirement is that it never panics.
    for index in 0..encoded.len() {
        for mask in [0x01u8, 0x80] {
            let mut corrupted = encoded.clone();
            if let Some(byte) = corrupted.get_mut(index) {
                *byte ^= mask;
            }
            if let Ok(decoded) = xpress8::decompress(&corrupted, input.len()) {
                assert_eq!(decoded.len(), input.len());
            }
        }
    }
}

#[test]
fn rejects_oversized_and_empty_chunks() {
    assert!(xpress8::decompress(&[0, 0, 0, 0], 0).is_err());
    assert!(xpress8::decompress(&[0u8; 8], xpress8::MAX_BLOCK + 1).is_err());
    assert!(xpress8::compress(&vec![0u8; xpress8::MAX_BLOCK + 1]).is_none());
}
