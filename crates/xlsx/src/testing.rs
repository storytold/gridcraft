//! Test support: builds password-encrypted workbooks in code, so tests here and in the crates
//! above (engine, UI) never need an encrypted file on disk. Compiled for this crate's tests and,
//! elsewhere, only with the `test-support` feature (a dev-dependency feature, never shipped).
//!
//! Salts and keys are fixed so the output is deterministic. This is not an encryptor for real
//! use: a fixed key protects nothing.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use aes::cipher::{BlockCipherEncrypt, KeyInit};

use crate::crypto::{
    BLOCK_HMAC_KEY, BLOCK_HMAC_VALUE, BLOCK_KEY, BLOCK_VERIFIER_HASH, BLOCK_VERIFIER_INPUT, F_AES, F_CRYPTOAPI, Hash, PASSWORD_ENCRYPTOR, Params,
    SEGMENT, derive, hmac, iterated_hash, standard_key,
};

/// The spin count `encrypt_workbook` uses: Excel's default.
pub const SPIN_COUNT: u32 = 100_000;

const PASSWORD_SALT: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
const KEY_DATA_SALT: [u8; 16] = [16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31];
const SECRET_KEY: [u8; 32] =
    [64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95];
const VERIFIER: [u8; 16] = [32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47];
/// The dataIntegrity HMAC key, as long as the hash: Excel writes 64 bytes for SHA-512, not the
/// salt-size key [MS-OFFCRYPTO] 2.3.4.14 describes.
const HMAC_KEY: [u8; 64] = [
    112, 113, 114, 115, 116, 117, 118, 119, 120, 121, 122, 123, 124, 125, 126, 127, 128, 129, 130, 131, 132, 133, 134, 135, 136, 137, 138, 139, 140,
    141, 142, 143, 144, 145, 146, 147, 148, 149, 150, 151, 152, 153, 154, 155, 156, 157, 158, 159, 160, 161, 162, 163, 164, 165, 166, 167, 168, 169,
    170, 171, 172, 173, 174, 175,
];

/// `package` (normally an `.xlsx` zip) as Excel stores it with a password: Agile encryption
/// (AES-256, SHA-512) inside a compound file.
pub fn encrypt_workbook(package: &[u8], password: &str) -> Vec<u8> {
    let (info, encrypted) = encrypt_agile(package, password, SPIN_COUNT);
    compound_file(&[("EncryptionInfo", &info), ("EncryptedPackage", &encrypted)])
}

/// The same with Standard encryption (AES-128, SHA-1), as Excel 2007 wrote it.
pub fn encrypt_workbook_standard(package: &[u8], password: &str) -> Vec<u8> {
    let (info, encrypted) = encrypt_standard(package, password);
    compound_file(&[("EncryptionInfo", &info), ("EncryptedPackage", &encrypted)])
}

/// A version 3 compound file (512-byte sectors) holding `streams` under the root, laid out as
/// Excel lays it out: streams under 4096 bytes in the mini stream, larger ones in regular
/// sectors. Up to 109 FAT sectors (about 7 MB of streams).
pub fn compound_file(streams: &[(&str, &[u8])]) -> Vec<u8> {
    const SECTOR: usize = 512;
    const MINI: usize = 64;
    const CUTOFF: usize = 4096;
    const FREE: u32 = 0xFFFF_FFFF;
    const END: u32 = 0xFFFF_FFFE;
    const FAT_SECTOR: u32 = 0xFFFF_FFFD;
    let is_mini = |d: &[u8]| d.len() < CUTOFF;

    // The mini stream: small streams in 64-byte mini sectors, chained by the mini FAT.
    let mut container = vec![];
    let mut mini_fat: Vec<u32> = vec![];
    let mut mini_starts = vec![];
    for (_, d) in streams {
        if !is_mini(d) || d.is_empty() {
            mini_starts.push(END);
            continue;
        }
        let first = mini_fat.len();
        let n = d.len().div_ceil(MINI);
        for k in 0..n {
            mini_fat.push(if k + 1 == n { END } else { (first + k + 1) as u32 });
        }
        mini_starts.push(first as u32);
        container.extend_from_slice(d);
        container.resize((first + n) * MINI, 0);
    }

    // Regular-sector blobs, in order: directory, mini FAT, mini stream, then each large stream.
    let dir_len = (streams.len() + 1).div_ceil(4) * SECTOR;
    let mut blobs: Vec<Vec<u8>> = vec![vec![0; dir_len], mini_fat.iter().flat_map(|v| v.to_le_bytes()).collect(), container.clone()];
    blobs.extend(streams.iter().map(|(_, d)| if is_mini(d) { vec![] } else { d.to_vec() }));
    let counts: Vec<usize> = blobs.iter().map(|b| b.len().div_ceil(SECTOR)).collect();
    let body: usize = counts.iter().sum();
    let mut fat_sectors = 1;
    while fat_sectors * 128 < fat_sectors + body {
        fat_sectors += 1;
    }
    assert!(fat_sectors <= 109, "the test writer has no DIFAT sectors");
    let mut fat = vec![FREE; fat_sectors * 128];
    fat[..fat_sectors].fill(FAT_SECTOR);
    let mut starts = vec![];
    let mut next = fat_sectors;
    for &n in &counts {
        if n == 0 {
            starts.push(END);
            continue;
        }
        starts.push(next as u32);
        for k in 0..n {
            fat[next + k] = if k + 1 == n { END } else { (next + k + 1) as u32 };
        }
        next += n;
    }

    let put16 = |b: &mut [u8], at: usize, v: u16| b[at..at + 2].copy_from_slice(&v.to_le_bytes());
    let put32 = |b: &mut [u8], at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    // Directory: the root (owning the mini stream), then the streams chained as right siblings.
    let dir = &mut blobs[0];
    for k in 0..dir_len / 128 {
        for at in [68, 72, 76] {
            put32(dir, k * 128 + at, FREE);
        }
    }
    let mut entry = |k: usize, name: &str, kind: u8, start: u32, size: usize| {
        let e = k * 128;
        let units: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
        dir[e..e + units.len()].copy_from_slice(&units);
        put16(dir, e + 64, units.len() as u16 + 2);
        dir[e + 66] = kind;
        dir[e + 67] = 1; // black
        if kind == 5 && !streams.is_empty() {
            put32(dir, e + 76, 1);
        }
        if kind == 2 && k < streams.len() {
            put32(dir, e + 72, k as u32 + 1);
        }
        put32(dir, e + 116, start);
        put32(dir, e + 120, size as u32);
    };
    entry(0, "Root Entry", 5, starts[2], container.len());
    for (k, (name, data)) in streams.iter().enumerate() {
        let start = if is_mini(data) { mini_starts[k] } else { starts[3 + k] };
        entry(k + 1, name, 2, start, data.len());
    }

    let mut out = vec![0u8; SECTOR * (1 + fat_sectors + body)];
    out[..8].copy_from_slice(&crate::cfb::SIGNATURE);
    put16(&mut out, 24, 0x3E); // minor version
    put16(&mut out, 26, 3); // major version
    put16(&mut out, 28, 0xFFFE); // byte order
    put16(&mut out, 30, 9); // 512-byte sectors
    put16(&mut out, 32, 6); // 64-byte mini sectors
    put32(&mut out, 44, fat_sectors as u32);
    put32(&mut out, 48, starts[0]);
    put32(&mut out, 56, CUTOFF as u32);
    put32(&mut out, 60, starts[1]); // first mini FAT sector (END when there is none)
    put32(&mut out, 64, counts[1] as u32);
    put32(&mut out, 68, END); // no DIFAT sectors
    for i in 0..109 {
        put32(&mut out, 76 + i * 4, if i < fat_sectors { i as u32 } else { FREE });
    }
    for (i, v) in fat.iter().enumerate() {
        put32(&mut out, SECTOR + i * 4, *v);
    }
    for (blob, start) in blobs.iter().zip(&starts) {
        if !blob.is_empty() {
            let at = SECTOR * (1 + *start as usize);
            out[at..at + blob.len()].copy_from_slice(blob);
        }
    }
    out
}

fn encrypt_block(key: &[u8], block: &mut [u8; 16]) {
    let b = aes::Block::cast_from_core_mut(block);
    match key.len() {
        16 => aes::Aes128::new_from_slice(key).unwrap().encrypt_block(b),
        32 => aes::Aes256::new_from_slice(key).unwrap().encrypt_block(b),
        n => panic!("no {n}-byte AES in the test encryptor"),
    }
}

fn pad(data: &[u8]) -> Vec<u8> {
    let mut v = data.to_vec();
    v.resize(data.len().div_ceil(16) * 16, 0);
    v
}

fn encrypt_ecb(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut v = pad(data);
    v.as_chunks_mut::<16>().0.iter_mut().for_each(|b| encrypt_block(key, b));
    v
}

fn encrypt_cbc(key: &[u8], iv: &[u8], data: &[u8]) -> Vec<u8> {
    let mut v = pad(data);
    let mut prev: [u8; 16] = iv.try_into().unwrap();
    for b in v.as_chunks_mut::<16>().0 {
        b.iter_mut().zip(prev).for_each(|(x, p)| *x ^= p);
        encrypt_block(key, b);
        prev = *b;
    }
    v
}

fn base64_encode(b: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in b.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            s.push(if i <= c.len() { T[(n >> (18 - 6 * i)) as usize & 63] as char } else { '=' });
        }
    }
    s
}

fn package(size: usize, body: Vec<u8>) -> Vec<u8> {
    let mut p = (size as u64).to_le_bytes().to_vec();
    p.extend(body);
    p
}

/// The Agile XML descriptor: AES-256, SHA-512, a password encryptor salted with 0..16, and the
/// dataIntegrity pair (`hmac_key_b64`/`hmac_value_b64`) a compliant file carries.
pub(crate) fn agile_info(password: &str, spin: u32, hmac_key_b64: &str, hmac_value_b64: &str) -> String {
    let key = Params { hash: Hash::Sha512, salt: PASSWORD_SALT.to_vec(), key_len: 32 };
    let h = iterated_hash(Hash::Sha512, password, &key.salt, spin);
    let iv = key.iv(None);
    let field = |block_key: &[u8], data: &[u8]| base64_encode(&encrypt_cbc(&derive(Hash::Sha512, &h, block_key, 32), &iv, data));
    let input = field(&BLOCK_VERIFIER_INPUT, &VERIFIER);
    let value = field(&BLOCK_VERIFIER_HASH, &Hash::Sha512.digest(&[&VERIFIER]));
    let secret = field(&BLOCK_KEY, &SECRET_KEY);
    let params = |salt: &[u8]| {
        format!(
            "saltSize=\"16\" blockSize=\"16\" keyBits=\"256\" hashSize=\"64\" cipherAlgorithm=\"AES\" cipherChaining=\"ChainingModeCBC\" hashAlgorithm=\"SHA512\" saltValue=\"{}\"",
            base64_encode(salt)
        )
    };
    // The dataIntegrity pair ([MS-OFFCRYPTO] 2.3.4.14), encrypted with the package key.
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<encryption xmlns=\"http://schemas.microsoft.com/office/2006/encryption\" xmlns:p=\"http://schemas.microsoft.com/office/2006/keyEncryptor/password\"><keyData {}/><dataIntegrity encryptedHmacKey=\"{hmac_key_b64}\" encryptedHmacValue=\"{hmac_value_b64}\"/><keyEncryptors><keyEncryptor uri=\"{PASSWORD_ENCRYPTOR}\"><p:encryptedKey spinCount=\"{spin}\" {} encryptedVerifierHashInput=\"{input}\" encryptedVerifierHashValue=\"{value}\" encryptedKeyValue=\"{secret}\"/></keyEncryptor></keyEncryptors></encryption>",
        params(KEY_DATA_SALT.as_slice()),
        params(&PASSWORD_SALT),
    )
}

/// `EncryptionInfo` for an Agile descriptor: version 4.4, reserved 0x40, then the XML.
pub(crate) fn with_agile_header(xml: &str) -> Vec<u8> {
    let mut info = vec![4, 0, 4, 0, 0x40, 0, 0, 0];
    info.extend_from_slice(xml.as_bytes());
    info
}

/// `EncryptionInfo` and `EncryptedPackage` for `plain`, Agile (AES-256, SHA-512), with the
/// dataIntegrity HMAC over the whole EncryptedPackage stream as Excel writes it.
pub(crate) fn encrypt_agile(plain: &[u8], password: &str, spin: u32) -> (Vec<u8>, Vec<u8>) {
    encrypt_agile_with_hmac_key(plain, password, spin, &HMAC_KEY)
}

/// [`encrypt_agile`] with the dataIntegrity HMAC keyed by `hmac_key`.
pub(crate) fn encrypt_agile_with_hmac_key(plain: &[u8], password: &str, spin: u32, hmac_key: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let key_data = Params { hash: Hash::Sha512, salt: KEY_DATA_SALT.to_vec(), key_len: 32 };
    let mut body = vec![];
    for (i, segment) in plain.chunks(SEGMENT).enumerate() {
        body.extend(encrypt_cbc(&SECRET_KEY, &key_data.iv(Some(&(i as u32).to_le_bytes())), segment));
    }
    let stream = package(plain.len(), body);
    let (hmac_key_b64, hmac_value_b64) = (
        base64_encode(&encrypt_cbc(&SECRET_KEY, &key_data.iv(Some(&BLOCK_HMAC_KEY)), hmac_key)),
        base64_encode(&encrypt_cbc(&SECRET_KEY, &key_data.iv(Some(&BLOCK_HMAC_VALUE)), &hmac(Hash::Sha512, hmac_key, &stream))),
    );
    (with_agile_header(&agile_info(password, spin, &hmac_key_b64, &hmac_value_b64)), stream)
}

/// `EncryptionInfo` and `EncryptedPackage` for `plain`, Standard (AES-128, SHA-1).
pub(crate) fn encrypt_standard(plain: &[u8], password: &str) -> (Vec<u8>, Vec<u8>) {
    let key = standard_key(password, &PASSWORD_SALT, 16);
    let mut header = vec![];
    for v in [F_CRYPTOAPI | F_AES, 0, 0x660E, 0x8004, 128, 0x18, 0, 0] {
        header.extend_from_slice(&u32::to_le_bytes(v));
    }
    header.extend("\0".encode_utf16().flat_map(u16::to_le_bytes));
    let mut info = vec![4, 0, 2, 0];
    info.extend_from_slice(&(F_CRYPTOAPI | F_AES).to_le_bytes());
    info.extend_from_slice(&(header.len() as u32).to_le_bytes());
    info.extend(header);
    info.extend_from_slice(&16u32.to_le_bytes());
    info.extend(PASSWORD_SALT);
    info.extend(encrypt_ecb(&key, &VERIFIER));
    info.extend_from_slice(&20u32.to_le_bytes());
    info.extend(encrypt_ecb(&key, &Hash::Sha1.digest(&[&VERIFIER])));
    (info, package(plain.len(), encrypt_ecb(&key, plain)))
}
