//! Decryption tests. The expected keys and IVs below were computed independently with
//! msoffcrypto-tool 6.0.0 (MIT) for the synthetic password and salts used here; the encrypted
//! files are built in code by `crate::testing`. No Excel-produced file is used.

use super::*;
use crate::testing::{agile_info, encrypt_agile, encrypt_standard, with_agile_header};

const PASSWORD: &str = "Pass1234";
/// 16 zero bytes (whole AES blocks), a stand-in for a real integrity field where the values
/// can't be reached anyway (scheme errors, descriptor-parse damage).
const HMAC_STUB: &str = "AAAAAAAAAAAAAAAAAAAAAA==";

fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

fn salt() -> Vec<u8> {
    (0u8..16).collect()
}

#[test]
fn aes_decrypts_the_fips_197_vectors() {
    let mut b: [u8; 16] = hex("69c4e0d86a7b0430d8cdb78070b4c55a").try_into().unwrap();
    Aes::new(&hex("000102030405060708090a0b0c0d0e0f")).unwrap().decrypt_block(&mut b);
    assert_eq!(b.to_vec(), hex("00112233445566778899aabbccddeeff"));
    let mut b: [u8; 16] = hex("8ea2b7ca516745bfeafc49904b496089").try_into().unwrap();
    Aes::new(&hex("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f")).unwrap().decrypt_block(&mut b);
    assert_eq!(b.to_vec(), hex("00112233445566778899aabbccddeeff"));
}

#[test]
fn agile_keys_match_msoffcrypto() {
    let h = iterated_hash(Hash::Sha512, PASSWORD, &salt(), 1000);
    assert_eq!(derive(Hash::Sha512, &h, &BLOCK_VERIFIER_INPUT, 32), hex("c57def04938e1e83497271f6ec099e890bfd2bd9cf05ff208f8b2405df361ae6"));
    assert_eq!(derive(Hash::Sha512, &h, &BLOCK_KEY, 32), hex("4ab08cf2ed95199d58d04f19331049a8a07cc1ff508431a1dd0785c84e261d03"));
    let h = iterated_hash(Hash::Sha1, PASSWORD, &salt(), 1000);
    assert_eq!(derive(Hash::Sha1, &h, &BLOCK_VERIFIER_INPUT, 16), hex("88c7567d028eae33a9a55f65729dcd2b"));
    let key_data = Params { hash: Hash::Sha512, salt: (16u8..32).collect(), key_len: 32 };
    assert_eq!(key_data.iv(Some(&1u32.to_le_bytes())), hex("da22ccba062aff577d207b2dee4e165b"));
}

#[test]
fn agile_pads_short_hashes_with_0x36() {
    // SHA-1 gives 20 bytes; an AES-256 key needs 32, so 12 bytes of 0x36 follow.
    let k = derive(Hash::Sha1, b"a", b"b", 32);
    assert_eq!(k[..20], Hash::Sha1.digest(&[b"ab"])[..]);
    assert_eq!(k[20..], [0x36; 12]);
}

#[test]
fn standard_keys_match_msoffcrypto() {
    assert_eq!(standard_key(PASSWORD, &salt(), 16), hex("ae0b402aeb23a779471a130843e431d4"));
    assert_eq!(standard_key(PASSWORD, &salt(), 32), hex("ae0b402aeb23a779471a130843e431d47b416314b5146b491cb815283663d26f"));
}

#[test]
fn base64_decodes_and_rejects_junk() {
    assert_eq!(base64("AAECAwQFBgcICQoLDA0ODw==").unwrap(), salt());
    assert_eq!(base64(" AAEC\nAw== ").unwrap(), vec![0, 1, 2, 3]);
    assert!(matches!(base64("AA*C"), Err(DecryptError::Malformed(_))));
}

/// A payload that isn't a whole number of segments or blocks.
fn payload(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 7 + i / 251) as u8).collect()
}

#[test]
fn agile_round_trips() {
    let plain = payload(10_000);
    let (info, package) = encrypt_agile(&plain, PASSWORD, 1000);
    assert_eq!(decrypt_streams(&info, &package, PASSWORD).unwrap(), plain);
}

#[test]
fn standard_round_trips() {
    let plain = payload(10_000);
    let (info, package) = encrypt_standard(&plain, PASSWORD);
    assert_eq!(decrypt_streams(&info, &package, PASSWORD).unwrap(), plain);
}

#[test]
fn non_ascii_passwords_round_trip() {
    // UTF-16LE, surrogate pairs included: "Pässwörd€" plus an emoji outside the BMP.
    let pw = "P\u{e4}ssw\u{f6}rd\u{20ac}\u{1F511}";
    let (info, package) = encrypt_agile(&payload(100), pw, 1000);
    assert_eq!(decrypt_streams(&info, &package, pw).unwrap(), payload(100));
    assert_eq!(decrypt_streams(&info, &package, "Passwrd"), Err(DecryptError::WrongPassword));
    let (info, package) = encrypt_standard(&payload(100), pw);
    assert_eq!(decrypt_streams(&info, &package, pw).unwrap(), payload(100));
}

#[test]
fn a_wrong_password_is_reported_as_such() {
    let (info, package) = encrypt_agile(&payload(100), PASSWORD, 1000);
    assert_eq!(decrypt_streams(&info, &package, "pass1234"), Err(DecryptError::WrongPassword));
    assert_eq!(decrypt_streams(&info, &package, ""), Err(DecryptError::WrongPassword));
    let (info, package) = encrypt_standard(&payload(100), PASSWORD);
    assert_eq!(decrypt_streams(&info, &package, "Pass12345"), Err(DecryptError::WrongPassword));
}

#[test]
fn unsupported_schemes_are_named() {
    let unsupported = |info: &[u8]| match decrypt_streams(info, &[0; 8], PASSWORD) {
        Err(DecryptError::Unsupported(s)) => s,
        other => panic!("expected Unsupported, got {other:?}"),
    };
    let (mut rc4, _) = encrypt_standard(&payload(10), PASSWORD);
    rc4[4..8].copy_from_slice(&F_CRYPTOAPI.to_le_bytes()); // fCryptoAPI without fAES
    assert_eq!(unsupported(&rc4), "RC4");
    assert_eq!(unsupported(&[3, 0, 3, 0, 0, 0, 0, 0]), "Extensible");
    assert_eq!(unsupported(&[1, 0, 1, 0, 0, 0, 0, 0]), "version 1.1");
    let des = agile_info(PASSWORD, 1000, HMAC_STUB, HMAC_STUB).replace("cipherAlgorithm=\"AES\"", "cipherAlgorithm=\"3DES\"");
    assert_eq!(unsupported(&with_agile_header(&des)), "3DES");
    let cert = agile_info(PASSWORD, 1000, HMAC_STUB, HMAC_STUB).replace("keyEncryptor/password", "keyEncryptor/certificate");
    assert_eq!(unsupported(&with_agile_header(&cert)), "certificate");
}

#[test]
fn damaged_files_are_malformed_not_panics() {
    let is_malformed = |r: Result<Vec<u8>, DecryptError>| matches!(r, Err(DecryptError::Malformed(_)));
    let plain = payload(5000);
    for (info, package) in [encrypt_agile(&plain, PASSWORD, 1000), encrypt_standard(&plain, PASSWORD)] {
        // Every truncation of either stream.
        for cut in (0..info.len()).step_by(7) {
            let _ = decrypt_streams(&info[..cut], &package, PASSWORD);
        }
        for cut in (0..package.len()).step_by(97) {
            let _ = decrypt_streams(&info, &package[..cut], PASSWORD);
        }
        // A stated size past the ciphertext never drives an allocation.
        let mut huge = package.clone();
        huge[..8].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(is_malformed(decrypt_streams(&info, &huge, PASSWORD)));
        // Ciphertext that isn't whole AES blocks.
        let mut ragged = package.clone();
        ragged.truncate(ragged.len() - 3);
        let len = (ragged.len() - 8) as u64;
        ragged[..8].copy_from_slice(&len.to_le_bytes());
        assert!(is_malformed(decrypt_streams(&info, &ragged, PASSWORD)));
    }
    let xml = agile_info(PASSWORD, 1000, HMAC_STUB, HMAC_STUB);
    let bad = |s: String| is_malformed(decrypt_streams(&with_agile_header(&s), &[0; 8], PASSWORD));
    assert!(bad(xml.replace("spinCount=\"1000\"", "spinCount=\"4000000000\"")));
    assert!(bad(xml.replace("saltValue=\"", "saltValue=\"*")));
    assert!(bad(xml.replace("keyBits=\"256\"", "keyBits=\"100\"")));
    assert!(bad(xml.replace("blockSize=\"16\"", "blockSize=\"8\"")));
    assert!(bad(xml.replace("<keyData", "<other")));
    assert!(bad("<encryption".into()));
}

#[test]
fn non_ascii_passwords_hash_as_utf16le_like_msoffcrypto() {
    // "Pässwörd€" plus an emoji outside the BMP, a surrogate pair in UTF-16.
    let h = iterated_hash(Hash::Sha512, "P\u{e4}ssw\u{f6}rd\u{20ac}\u{1F511}", &salt(), 1000);
    assert_eq!(
        h,
        hex("2bca96ca0a7e8d88d9314a2eb1819a1ece94ff9ce34266ff82f891a88c4240c4c9f39c729b7c5c410c27fc8e2b7ad90d8c005eb68a603377b8a4ab858041c76a")
    );
}

#[test]
fn hmac_matches_the_rfc2104_test_vectors() {
    use crate::crypto::hmac as mac;
    let h = |v: Vec<u8>| v.iter().map(|b| format!("{b:02x}")).collect::<String>();
    // RFC 2202 test case 1 (HMAC-SHA-1) and HMAC-SHA-256 vectors.
    assert_eq!(h(mac(Hash::Sha1, &[0x0b; 20], b"Hi There")), "b617318655057264e28bc0b6fb378c8ef146be00");
    assert_eq!(h(mac(Hash::Sha256, &[0x0b; 20], b"Hi There")), "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7");
    // RFC 4231 test case 5 (HMAC-SHA-256): a key longer than the block size is hashed first.
    assert_eq!(
        h(mac(Hash::Sha256, &[0xaa; 131], b"Test Using Larger Than Block-Size Key - Hash Key First")),
        "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
    );
}

/// Strips the `<dataIntegrity …/>` element from an Agile descriptor.
fn without_integrity(info: &[u8]) -> Vec<u8> {
    let xml = std::str::from_utf8(&info[8..]).unwrap().to_string();
    let start = xml.find("<dataIntegrity").unwrap();
    let end = start + xml[start..].find("/>").unwrap() + 2;
    let stripped = format!("{}{}", &xml[..start], &xml[end..]);
    with_agile_header(&stripped)
}

#[test]
fn integrity_is_required_and_enforced() {
    let plain = payload(300);
    let (info, package) = encrypt_agile(&plain, PASSWORD, 1000);
    // An attacker can't just delete the integrity element to get the package verified.
    assert!(matches!(decrypt_streams(&without_integrity(&info), &package, PASSWORD),
                     Err(DecryptError::Malformed(m)) if m.contains("dataIntegrity")));
    // An empty or ragged field is damage, not a pass.
    for bad in [("encryptedHmacKey", ""), ("encryptedHmacKey", "AA=="), ("encryptedHmacValue", "")] {
        let xml =
            agile_info(PASSWORD, 1000, HMAC_STUB, HMAC_STUB).replace(&format!("{}=\"{HMAC_STUB}\"", bad.0), &format!("{}=\"{}\"", bad.0, bad.1));
        let r = decrypt_streams(&with_agile_header(&xml), &package, PASSWORD);
        assert!(matches!(r, Err(DecryptError::Malformed(_))), "{}=\"{}\"", bad.0, bad.1);
    }
}

#[test]
fn modified_ciphertext_fails_the_integrity_check() {
    let plain = payload(300);
    let (info, package) = encrypt_agile(&plain, PASSWORD, 1000);
    // The correct password proves the verifier can't vouch for a modified package.
    for at in [8usize, 100, package.len() - 5] {
        let mut tampered = package.clone();
        tampered[at] ^= 0x80;
        assert!(
            matches!(decrypt_streams(&info, &tampered, PASSWORD),
                         Err(DecryptError::Malformed(m)) if m.contains("integrity")),
            "byte {at}"
        );
    }
    // And neither can the descriptor: a swapped integrity value is refused.
    let xml = String::from_utf8_lossy(&info[8..]).to_string();
    let at = xml.find("encryptedHmacValue=\"").unwrap() + "encryptedHmacValue=\"".len();
    let mut xml: Vec<u8> = xml.into_bytes();
    xml[at] ^= b'A' ^ b'B';
    assert!(matches!(decrypt_streams(&with_agile_header(std::str::from_utf8(&xml).unwrap()), &package, PASSWORD), Err(DecryptError::Malformed(_))));
}

#[test]
fn integrity_matches_the_stream_not_the_plaintext() {
    // The MAC covers the whole EncryptedPackage stream, StreamSize field included: changing the
    // stated size alone must fail, while the actual bytes round-trip.
    let plain = payload(4200);
    let (info, package) = encrypt_agile(&plain, PASSWORD, 1000);
    let mut ours = package.clone();
    ours[..8].copy_from_slice(&((plain.len() + 1) as u64).to_le_bytes());
    assert!(matches!(decrypt_streams(&info, &ours, PASSWORD), Err(DecryptError::Malformed(m)) if m.contains("integrity")));
    assert_eq!(decrypt_streams(&info, &package, PASSWORD).unwrap(), plain);
}
