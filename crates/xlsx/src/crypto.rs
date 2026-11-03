//! Password-encrypted workbooks. Excel wraps the ordinary OOXML package in a compound file with
//! two streams: `EncryptionInfo` (how the key comes from the password, and a verifier that tells
//! a wrong password) and `EncryptedPackage` (the package, encrypted with AES). Agile encryption
//! (Excel 2010 onward) and Standard encryption (Excel 2007) are read; RC4, Extensible and
//! certificate-only encryption are reported as unsupported.
//!
//! Clean-room from Microsoft's public [MS-OFFCRYPTO] specification. Every read is
//! bounds-checked, and the costly inputs (spin count, stated sizes) are capped.

use aes::cipher::{BlockCipherDecrypt, KeyInit};
use sha2::Digest;

use crate::cfb::Compound;
use crate::xml::El;

/// Spin counts above this are refused (the cap sheet-protection hashes use too).
const MAX_SPIN_COUNT: u32 = 10_000_000;
/// Standard encryption always iterates this many times.
const STANDARD_SPIN_COUNT: u32 = 50_000;
/// Agile packages are encrypted in segments of this many bytes, each with its own IV.
pub(crate) const SEGMENT: usize = 4096;
/// AES block size.
const BLOCK: usize = 16;

/// Agile block keys ([MS-OFFCRYPTO] 2.3.4.13): one per value derived from the password.
pub(crate) const BLOCK_VERIFIER_INPUT: [u8; 8] = [0xFE, 0xA7, 0xD2, 0x76, 0x3B, 0x4B, 0x9E, 0x79];
pub(crate) const BLOCK_VERIFIER_HASH: [u8; 8] = [0xD7, 0xAA, 0x0F, 0x6D, 0x30, 0x61, 0x34, 0x4E];
pub(crate) const BLOCK_KEY: [u8; 8] = [0x14, 0x6E, 0x0B, 0xE7, 0xAB, 0xAC, 0xD0, 0xD6];
/// The two dataIntegrity block keys ([MS-OFFCRYPTO] 2.3.4.14), one per HMAC field.
pub(crate) const BLOCK_HMAC_KEY: [u8; 8] = [0x5F, 0xB2, 0xAD, 0x01, 0x0C, 0xB9, 0xE1, 0xF6];
pub(crate) const BLOCK_HMAC_VALUE: [u8; 8] = [0xA0, 0x67, 0x7F, 0x02, 0xB2, 0x2C, 0x84, 0x33];

/// The password key encryptor's URI (the other kind is certificate-based).
pub(crate) const PASSWORD_ENCRYPTOR: &str = "http://schemas.microsoft.com/office/2006/keyEncryptor/password";

/// Standard encryption header flags ([MS-OFFCRYPTO] 2.3.1).
pub(crate) const F_CRYPTOAPI: u32 = 0x04;
const F_EXTERNAL: u32 = 0x10;
pub(crate) const F_AES: u32 = 0x20;

/// Why an encrypted workbook couldn't be decrypted.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DecryptError {
    #[error("the password is incorrect")]
    WrongPassword,
    /// The scheme, e.g. "RC4" or "Extensible", to complete "uses {0} encryption".
    #[error("{0} encryption isn't supported")]
    Unsupported(String),
    #[error("the encrypted file is damaged ({0})")]
    Malformed(String),
}

fn malformed(m: impl Into<String>) -> DecryptError {
    DecryptError::Malformed(m.into())
}

/// The plain OOXML package (a zip) inside an encrypted workbook.
pub fn decrypt(bytes: &[u8], password: &str) -> Result<Vec<u8>, DecryptError> {
    let (info, package) = streams(bytes)?;
    decrypt_streams(&info, &package, password)
}

/// Whether the right password could decrypt this encrypted workbook, checked without one. An
/// error names a scheme no password opens (RC4, rights management…) or damage, so callers can
/// say so instead of asking for a password first.
pub fn check_encryption(bytes: &[u8]) -> Result<(), DecryptError> {
    let (info, _) = streams(bytes)?;
    match scheme(&info)? {
        Scheme::Agile(xml) => agile_descriptor(xml).map(|_| ()),
        Scheme::Standard(info) => standard_header(info).map(|_| ()),
    }
}

/// The `EncryptionInfo` and `EncryptedPackage` streams. Rights management (IRM) keeps the
/// package but describes its encryption elsewhere, with no `EncryptionInfo`.
fn streams(bytes: &[u8]) -> Result<(Vec<u8>, Vec<u8>), DecryptError> {
    let cf = Compound::open(bytes).map_err(malformed)?;
    let info = cf.root_stream("EncryptionInfo").map_err(malformed)?;
    let package = cf.root_stream("EncryptedPackage").map_err(malformed)?.ok_or_else(|| malformed("no EncryptedPackage stream"))?;
    let info = info.ok_or_else(|| DecryptError::Unsupported("rights-management".into()))?;
    Ok((info, package))
}

/// Decrypts from the two streams' contents.
pub(crate) fn decrypt_streams(info: &[u8], package: &[u8], password: &str) -> Result<Vec<u8>, DecryptError> {
    match scheme(info)? {
        Scheme::Agile(xml) => agile(xml, package, password),
        Scheme::Standard(info) => standard(info, package, password),
    }
}

/// The two schemes read, with the part of `EncryptionInfo` each one parses.
enum Scheme<'a> {
    /// The XML descriptor after the version and reserved field.
    Agile(&'a [u8]),
    /// Everything after the version.
    Standard(&'a [u8]),
}

fn scheme(info: &[u8]) -> Result<Scheme<'_>, DecryptError> {
    let short = || malformed("truncated EncryptionInfo");
    let major = u16_at(info, 0).ok_or_else(short)?;
    let minor = u16_at(info, 2).ok_or_else(short)?;
    match (major, minor) {
        (4, 4) => Ok(Scheme::Agile(info.get(8..).ok_or_else(short)?)),
        (2..=4, 2) => Ok(Scheme::Standard(info.get(4..).ok_or_else(short)?)),
        (3 | 4, 3) => Err(DecryptError::Unsupported("Extensible".into())),
        _ => Err(DecryptError::Unsupported(format!("version {major}.{minor}"))),
    }
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    let s = b.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([*s.first()?, *s.get(1)?]))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([*s.first()?, *s.get(1)?, *s.get(2)?, *s.get(3)?]))
}

/// The plaintext size an `EncryptedPackage` states, and the ciphertext after it. A size larger
/// than the ciphertext is damage, so it never drives an allocation.
fn package_parts(package: &[u8]) -> Result<(usize, &[u8]), DecryptError> {
    let head: [u8; 8] = package.get(..8).and_then(|h| h.try_into().ok()).ok_or_else(|| malformed("truncated EncryptedPackage"))?;
    let data = package.get(8..).unwrap_or_default();
    let size = usize::try_from(u64::from_le_bytes(head)).unwrap_or(usize::MAX);
    if size > data.len() {
        return Err(malformed("the package is shorter than its stated size"));
    }
    Ok((size, data))
}

// ---- hashing -------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Hash {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

impl Hash {
    fn from_name(name: &str) -> Result<Hash, DecryptError> {
        Ok(match name {
            "SHA1" | "SHA-1" => Hash::Sha1,
            "SHA256" | "SHA-256" => Hash::Sha256,
            "SHA384" | "SHA-384" => Hash::Sha384,
            "SHA512" | "SHA-512" => Hash::Sha512,
            other => return Err(DecryptError::Unsupported(format!("{other} hash"))),
        })
    }

    /// The digest of the concatenated `parts`.
    pub(crate) fn digest(self, parts: &[&[u8]]) -> Vec<u8> {
        fn run<D: Digest>(parts: &[&[u8]]) -> Vec<u8> {
            let mut d = D::new();
            for p in parts {
                d.update(p);
            }
            d.finalize().to_vec()
        }
        match self {
            Hash::Sha1 => run::<sha1::Sha1>(parts),
            Hash::Sha256 => run::<sha2::Sha256>(parts),
            Hash::Sha384 => run::<sha2::Sha384>(parts),
            Hash::Sha512 => run::<sha2::Sha512>(parts),
        }
    }

    /// The bytes the algorithm's compression function takes: 128 for SHA-384/512, else 64.
    fn block_size(self) -> usize {
        match self {
            Hash::Sha384 | Hash::Sha512 => 128,
            Hash::Sha1 | Hash::Sha256 => 64,
        }
    }
}

/// H0 = H(salt ‖ UTF-16LE(password)), then Hn = H(LE32(n - 1) ‖ Hn-1) for `spin` rounds.
pub(crate) fn iterated_hash(hash: Hash, password: &str, salt: &[u8], spin: u32) -> Vec<u8> {
    let pw: Vec<u8> = password.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut h = hash.digest(&[salt, &pw]);
    for i in 0..spin {
        h = hash.digest(&[&i.to_le_bytes(), &h]);
    }
    h
}

/// H(a ‖ b), truncated or padded with 0x36 to `len`: Agile's derived keys and IVs.
pub(crate) fn derive(hash: Hash, a: &[u8], b: &[u8], len: usize) -> Vec<u8> {
    let mut k = hash.digest(&[a, b]);
    k.resize(len, 0x36);
    k
}

/// HMAC-SHA-`hash` ([RFC2104]), as Agile's dataIntegrity uses.
pub(crate) fn hmac(hash: Hash, key: &[u8], message: &[u8]) -> Vec<u8> {
    let bs = hash.block_size();
    let mut k = key.to_vec();
    if k.len() > bs {
        k = hash.digest(&[&k]);
    }
    k.resize(bs, 0);
    let pad = |x: u8| k.iter().map(|b| b ^ x).collect::<Vec<u8>>();
    let inner = hash.digest(&[&pad(0x36), message]);
    hash.digest(&[&pad(0x5C), &inner])
}

/// The Standard encryption key ([MS-OFFCRYPTO] 2.3.4.7): SHA-1 iterated 50,000 times, the
/// block-0 hash, then the 0x36 / 0x5C expansion, truncated to `key_len`.
pub(crate) fn standard_key(password: &str, salt: &[u8], key_len: usize) -> Vec<u8> {
    let h = iterated_hash(Hash::Sha1, password, salt, STANDARD_SPIN_COUNT);
    let h = Hash::Sha1.digest(&[&h, &0u32.to_le_bytes()]);
    let expand = |pad: u8| {
        let mut buf = [pad; 64];
        for (b, x) in buf.iter_mut().zip(&h) {
            *b ^= x;
        }
        Hash::Sha1.digest(&[&buf])
    };
    let mut key = expand(0x36);
    key.extend(expand(0x5C));
    key.truncate(key_len);
    key
}

// ---- AES -----------------------------------------------------------------------------------

enum Aes {
    A128(aes::Aes128),
    A192(aes::Aes192),
    A256(aes::Aes256),
}

impl Aes {
    fn new(key: &[u8]) -> Result<Aes, DecryptError> {
        let bad = |_| malformed("bad AES key");
        Ok(match key.len() {
            16 => Aes::A128(aes::Aes128::new_from_slice(key).map_err(bad)?),
            24 => Aes::A192(aes::Aes192::new_from_slice(key).map_err(bad)?),
            32 => Aes::A256(aes::Aes256::new_from_slice(key).map_err(bad)?),
            n => return Err(malformed(format!("a {n}-byte AES key"))),
        })
    }

    fn decrypt_block(&self, block: &mut [u8; BLOCK]) {
        let b = aes::Block::cast_from_core_mut(block);
        match self {
            Aes::A128(c) => c.decrypt_block(b),
            Aes::A192(c) => c.decrypt_block(b),
            Aes::A256(c) => c.decrypt_block(b),
        }
    }

    /// ECB: each block on its own. `data` must be whole blocks.
    fn decrypt_ecb(&self, data: &mut [u8]) -> Result<(), DecryptError> {
        let (blocks, rest) = data.as_chunks_mut::<BLOCK>();
        if !rest.is_empty() {
            return Err(malformed("ciphertext isn't whole AES blocks"));
        }
        blocks.iter_mut().for_each(|b| self.decrypt_block(b));
        Ok(())
    }

    /// CBC: each block XORed with the previous ciphertext block (the IV for the first).
    fn decrypt_cbc(&self, iv: &[u8], data: &mut [u8]) -> Result<(), DecryptError> {
        let mut prev: [u8; BLOCK] = iv.try_into().map_err(|_| malformed("an IV that isn't one AES block"))?;
        let (blocks, rest) = data.as_chunks_mut::<BLOCK>();
        if !rest.is_empty() {
            return Err(malformed("ciphertext isn't whole AES blocks"));
        }
        for b in blocks {
            let cipher = *b;
            self.decrypt_block(b);
            b.iter_mut().zip(prev).for_each(|(x, p)| *x ^= p);
            prev = cipher;
        }
        Ok(())
    }
}

// ---- Standard encryption -------------------------------------------------------------------

/// What a Standard `EncryptionInfo` holds: the key size and the password verifier.
struct StandardHeader<'a> {
    key_len: usize,
    salt: &'a [u8],
    verifier: &'a [u8],
    verifier_hash: &'a [u8],
}

/// `info` starts after the version: flags, header size, header, verifier.
fn standard_header(info: &[u8]) -> Result<StandardHeader<'_>, DecryptError> {
    let short = || malformed("truncated Standard encryption header");
    let flags = u32_at(info, 0).ok_or_else(short)?;
    if flags & F_EXTERNAL != 0 {
        return Err(DecryptError::Unsupported("external".into()));
    }
    if flags & F_AES == 0 || flags & F_CRYPTOAPI == 0 {
        return Err(DecryptError::Unsupported("RC4".into()));
    }
    let header_size = usize::try_from(u32_at(info, 4).ok_or_else(short)?).map_err(|_| short())?;
    let header = info.get(8..8usize.checked_add(header_size).ok_or_else(short)?).ok_or_else(short)?;
    let alg_id = u32_at(header, 8).ok_or_else(short)?;
    let alg_hash = u32_at(header, 12).ok_or_else(short)?;
    let key_bits = u32_at(header, 16).ok_or_else(short)?;
    let key_len = match (alg_id, key_bits) {
        (0x660E | 0, 128) => 16,
        (0x660F, 192) => 24,
        (0x6610, 256) => 32,
        (0x660E..=0x6610, _) => return Err(malformed(format!("AES with a {key_bits}-bit key"))),
        _ => return Err(DecryptError::Unsupported(format!("cipher 0x{alg_id:04X}"))),
    };
    if alg_hash != 0x8004 && alg_hash != 0 {
        return Err(DecryptError::Unsupported(format!("hash 0x{alg_hash:04X}")));
    }
    // Verifier: salt size, salt, encrypted verifier, verifier hash size, encrypted hash.
    let v = info.get(8 + header_size..).ok_or_else(short)?;
    if u32_at(v, 0) != Some(16) {
        return Err(malformed("a Standard salt that isn't 16 bytes"));
    }
    let salt = v.get(4..20).ok_or_else(short)?;
    let verifier = v.get(20..36).ok_or_else(short)?;
    let verifier_hash = v.get(40..72).ok_or_else(short)?;
    Ok(StandardHeader { key_len, salt, verifier, verifier_hash })
}

fn standard(info: &[u8], package: &[u8], password: &str) -> Result<Vec<u8>, DecryptError> {
    let h = standard_header(info)?;
    let (mut verifier, mut verifier_hash) = (h.verifier.to_vec(), h.verifier_hash.to_vec());
    let aes = Aes::new(&standard_key(password, h.salt, h.key_len))?;
    aes.decrypt_ecb(&mut verifier)?;
    aes.decrypt_ecb(&mut verifier_hash)?;
    if verifier_hash.get(..20) != Some(Hash::Sha1.digest(&[&verifier]).as_slice()) {
        return Err(DecryptError::WrongPassword);
    }
    let (size, data) = package_parts(package)?;
    let mut out = data.get(..size.div_ceil(BLOCK) * BLOCK).ok_or_else(|| malformed("the package isn't whole AES blocks"))?.to_vec();
    aes.decrypt_ecb(&mut out)?;
    out.truncate(size);
    Ok(out)
}

// ---- Agile encryption ----------------------------------------------------------------------

/// The cipher parameters shared by `keyData` and a password `encryptedKey` element.
pub(crate) struct Params {
    pub(crate) hash: Hash,
    pub(crate) salt: Vec<u8>,
    pub(crate) key_len: usize,
}

impl Params {
    fn read(e: &El) -> Result<Params, DecryptError> {
        let cipher = e.attr("cipherAlgorithm").unwrap_or("AES");
        if cipher != "AES" {
            return Err(DecryptError::Unsupported(cipher.into()));
        }
        let chaining = e.attr("cipherChaining").unwrap_or("ChainingModeCBC");
        if chaining != "ChainingModeCBC" {
            return Err(DecryptError::Unsupported(format!("AES {chaining}")));
        }
        if e.attr_u32("blockSize") != Some(16) {
            return Err(malformed("an AES block size that isn't 16"));
        }
        let key_len = match e.attr_u32("keyBits") {
            Some(128) => 16,
            Some(192) => 24,
            Some(256) => 32,
            other => return Err(malformed(format!("AES with keyBits {}", other.unwrap_or(0)))),
        };
        let hash = Hash::from_name(e.attr("hashAlgorithm").unwrap_or("SHA1"))?;
        let salt = base64(e.attr("saltValue").ok_or_else(|| malformed("no saltValue"))?)?;
        if salt.is_empty() || salt.len() > 64 {
            return Err(malformed("a salt that isn't 1 to 64 bytes"));
        }
        Ok(Params { hash, salt, key_len })
    }

    /// An IV: H(salt ‖ block key) for a block key, else the salt, sized to one AES block.
    pub(crate) fn iv(&self, block_key: Option<&[u8]>) -> Vec<u8> {
        match block_key {
            Some(k) => derive(self.hash, &self.salt, k, BLOCK),
            None => {
                let mut iv = self.salt.clone();
                iv.resize(BLOCK, 0x36);
                iv
            }
        }
    }
}

/// What an Agile descriptor holds: the package's cipher, the password key encryptor with its
/// three encrypted values (verifier input, verifier hash, package key), and the package's
/// integrity pair, which Excel always writes and the reader requires.
struct AgileDescriptor {
    key_data: Params,
    key: Params,
    spin: u32,
    verifier_input: Vec<u8>,
    verifier_hash: Vec<u8>,
    key_value: Vec<u8>,
    hmac_key: Vec<u8>,
    hmac_value: Vec<u8>,
}

/// `xml` is the XML encryption descriptor after the 8-byte version and reserved field.
fn agile_descriptor(xml: &[u8]) -> Result<AgileDescriptor, DecryptError> {
    let root = crate::xml::parse(xml).map_err(|e| malformed(e.to_string()))?;
    let key_data = Params::read(root.child("keyData").ok_or_else(|| malformed("no keyData"))?)?;
    let encryptor = root
        .child("keyEncryptors")
        .and_then(|k| k.kids("keyEncryptor").find(|e| e.attr("uri") == Some(PASSWORD_ENCRYPTOR)))
        .ok_or_else(|| DecryptError::Unsupported("certificate".into()))?
        .child("encryptedKey")
        .ok_or_else(|| malformed("no encryptedKey"))?;
    let key = Params::read(encryptor)?;
    let spin = encryptor.attr_u32("spinCount").ok_or_else(|| malformed("no spinCount"))?;
    if spin > MAX_SPIN_COUNT {
        return Err(malformed(format!("a spin count of {spin}, more than the {MAX_SPIN_COUNT} allowed")));
    }
    let field = |name: &str| base64(encryptor.attr(name).ok_or_else(|| malformed(format!("no {name}")))?);
    // A descriptor without dataIntegrity can't prove its ciphertext is the one the password
    // protects: the verifier only authenticates the key, not the package ([MS-OFFCRYPTO] 2.3.4.14).
    let integrity = root.child("dataIntegrity").ok_or_else(|| malformed("no dataIntegrity"))?;
    let hmac_field = |attr: &str| {
        let v = base64(integrity.attr(attr).ok_or_else(|| malformed(format!("no {attr}")))?)?;
        if v.is_empty() || v.len() % BLOCK != 0 {
            return Err(malformed(format!("a whole-block {attr}")));
        }
        Ok(v)
    };
    Ok(AgileDescriptor {
        key_data,
        key,
        spin,
        verifier_input: field("encryptedVerifierHashInput")?,
        verifier_hash: field("encryptedVerifierHashValue")?,
        key_value: field("encryptedKeyValue")?,
        hmac_key: hmac_field("encryptedHmacKey")?,
        hmac_value: hmac_field("encryptedHmacValue")?,
    })
}

fn agile(xml: &[u8], package: &[u8], password: &str) -> Result<Vec<u8>, DecryptError> {
    let descriptor = agile_descriptor(xml)?;
    let AgileDescriptor { key_data, key, spin, verifier_input, verifier_hash, key_value, hmac_key, hmac_value } = descriptor;
    let h = iterated_hash(key.hash, password, &key.salt, spin);
    let iv = key.iv(None);
    let field = |data: &[u8], block_key: &[u8]| -> Result<Vec<u8>, DecryptError> {
        let mut data = data.to_vec();
        Aes::new(&derive(key.hash, &h, block_key, key.key_len))?.decrypt_cbc(&iv, &mut data)?;
        Ok(data)
    };
    let input = field(&verifier_input, &BLOCK_VERIFIER_INPUT)?;
    let expected = field(&verifier_hash, &BLOCK_VERIFIER_HASH)?;
    let input = input.get(..key.salt.len()).unwrap_or(&input);
    let actual = key.hash.digest(&[input]);
    if expected.get(..actual.len()) != Some(actual.as_slice()) {
        return Err(DecryptError::WrongPassword);
    }
    let secret = field(&key_value, &BLOCK_KEY)?;
    let aes = Aes::new(secret.get(..key_data.key_len).ok_or_else(|| malformed("a short encryptedKeyValue"))?)?;
    // Package integrity, [MS-OFFCRYPTO] 2.3.4.14: both HMAC fields are encrypted with the
    // package key (the verifier above only authenticates the password), keyed by a random
    // salt-size array, computed over the whole EncryptedPackage stream, StreamSize field included.
    let mut mac_field = hmac_key;
    aes.decrypt_cbc(&key_data.iv(Some(&BLOCK_HMAC_KEY)), &mut mac_field)?;
    mac_field.truncate(key_data.salt.len());
    let mut declared = hmac_value;
    aes.decrypt_cbc(&key_data.iv(Some(&BLOCK_HMAC_VALUE)), &mut declared)?;
    let mac = hmac(key_data.hash, &mac_field, package);
    if declared.get(..mac.len()) != Some(mac.as_slice()) {
        return Err(malformed("the decrypted data failed its integrity check"));
    }
    let (size, data) = package_parts(package)?;
    let mut out = Vec::with_capacity(size);
    for (i, segment) in data.chunks(SEGMENT).enumerate() {
        if out.len() >= size {
            break;
        }
        let index = u32::try_from(i).map_err(|_| malformed("too many segments"))?;
        let start = out.len();
        out.extend_from_slice(segment);
        let block = out.get_mut(start..).unwrap_or_default();
        aes.decrypt_cbc(&key_data.iv(Some(&index.to_le_bytes())), block)?;
    }
    out.truncate(size);
    Ok(out)
}

// ---- base64 --------------------------------------------------------------------------------

/// Standard base64 (whitespace ignored), as Agile's XML uses for every binary value.
fn base64(s: &str) -> Result<Vec<u8>, DecryptError> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let (mut buf, mut bits) = (0u32, 0u32);
    for c in s.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return Err(malformed("invalid base64")),
        };
        buf = (buf << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
