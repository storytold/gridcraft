//! Password-encrypted workbooks. Excel wraps the ordinary OOXML package in a compound file with
//! two streams: `EncryptionInfo` (how the key comes from the password, and a verifier that tells
//! a wrong password) and `EncryptedPackage` (the package, encrypted with AES). Agile encryption
//! (Excel 2010 onward) and Standard encryption (Excel 2007) are read; RC4, Extensible and
//! certificate-only encryption are reported as unsupported. Workbooks are written with Agile
//! encryption, as Excel saves a workbook with a password to open.
//!
//! Clean-room from Microsoft's public [MS-OFFCRYPTO] specification. Every read is
//! bounds-checked, and the costly inputs (spin count, stated sizes) are capped.

use aes::cipher::{BlockCipherDecrypt, BlockCipherEncrypt, KeyInit};
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
/// Agile block keys for the data-integrity HMAC's key and value ([MS-OFFCRYPTO] 2.3.4.14).
const BLOCK_HMAC_KEY: [u8; 8] = [0x5F, 0xB2, 0xAD, 0x01, 0x0C, 0xB9, 0xE1, 0xF6];
const BLOCK_HMAC_VALUE: [u8; 8] = [0xA0, 0x67, 0x7F, 0x02, 0xB2, 0x2C, 0x84, 0x33];
/// The spin count written: Excel's default.
const WRITE_SPIN_COUNT: u32 = 100_000;

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

/// Why a workbook couldn't be encrypted.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EncryptError {
    #[error("no secure random numbers are available ({0})")]
    Random(String),
    #[error("the workbook is too large to encrypt ({0})")]
    TooLarge(String),
}

fn malformed(m: impl Into<String>) -> DecryptError {
    DecryptError::Malformed(m.into())
}

/// The plain OOXML package (a zip) inside an encrypted workbook.
pub fn decrypt(bytes: &[u8], password: &str) -> Result<Vec<u8>, DecryptError> {
    let (info, package) = streams(bytes)?;
    decrypt_streams(&info, &package, password)
}

/// `package` (an `.xlsx` or `.xlsm` zip) encrypted with `password` the way Excel saves a
/// workbook with a password to open: Agile encryption (AES-256, SHA-512, 100,000 spins) in a
/// compound file, with fresh random salts and key every time.
pub fn encrypt(package: &[u8], password: &str) -> Result<Vec<u8>, EncryptError> {
    let (info, encrypted) = encrypt_streams(package, password, WRITE_SPIN_COUNT, &Secrets::random()?)?;
    let spaces = data_spaces();
    let mut streams: Vec<(&str, &[u8])> = spaces.iter().map(|(name, data)| (*name, data.as_slice())).collect();
    streams.push(("EncryptionInfo", &info));
    streams.push(("EncryptedPackage", &encrypted));
    crate::cfb::write(&streams).map_err(EncryptError::TooLarge)
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

/// The `\x06DataSpaces` storage ([MS-OFFCRYPTO] 2.1.5, 2.2): it declares the encryption
/// transform applied to `EncryptedPackage`, the same for every password-encrypted workbook.
fn data_spaces() -> [(&'static str, Vec<u8>); 4] {
    // UNICODE-LP-P4: the byte length, then UTF-16LE, padded to 4 bytes.
    let text = |t: &str| {
        let units: Vec<u8> = t.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let mut v = u32::try_from(units.len()).unwrap_or(0).to_le_bytes().to_vec();
        v.extend(units);
        v.resize(v.len().div_ceil(4) * 4, 0);
        v
    };
    let words = |w: &[usize]| -> Vec<u8> { w.iter().flat_map(|v| u32::try_from(*v).unwrap_or(0).to_le_bytes()).collect() };
    // Reader, updater and writer versions, each 1.0.
    let versions = words(&[1, 1, 1]);
    let version = [text("Microsoft.Container.DataSpaces"), versions.clone()].concat();
    // One map entry: the EncryptedPackage stream in StrongEncryptionDataSpace.
    let entry = [words(&[1, 0]), text("EncryptedPackage"), text("StrongEncryptionDataSpace")].concat();
    let map = [words(&[8, 1, entry.len() + 4]), entry].concat();
    let space = [words(&[8, 1]), text("StrongEncryptionTransform")].concat();
    let id = text("{FF9A3F03-56EF-4613-BDD5-5A41C1D07246}");
    // The transform header (its length counts up to the name), then an empty encryption name,
    // block size 0, cipher mode 0 and the reserved 4.
    let primary = [words(&[8 + id.len(), 1]), id, text("Microsoft.Container.EncryptionTransform"), versions, words(&[0, 0, 0, 4])].concat();
    [
        ("\u{6}DataSpaces/Version", version),
        ("\u{6}DataSpaces/DataSpaceMap", map),
        ("\u{6}DataSpaces/DataSpaceInfo/StrongEncryptionDataSpace", space),
        ("\u{6}DataSpaces/TransformInfo/StrongEncryptionTransform/\u{6}Primary", primary),
    ]
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

/// HMAC-SHA-512 (RFC 2104): Agile's data-integrity check.
fn hmac_sha512(key: &[u8], message: &[u8]) -> Vec<u8> {
    const SHA512_BLOCK: usize = 128;
    let mut k = if key.len() > SHA512_BLOCK { Hash::Sha512.digest(&[key]) } else { key.to_vec() };
    k.resize(SHA512_BLOCK, 0);
    let pad = |x: u8| k.iter().map(|b| b ^ x).collect::<Vec<u8>>();
    let inner = Hash::Sha512.digest(&[&pad(0x36), message]);
    Hash::Sha512.digest(&[&pad(0x5C), &inner])
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

    /// AES-256 with a 32-byte `key` (a shorter one would be zero-padded; callers pass 32).
    fn aes256(key: &[u8]) -> Aes {
        let mut k = [0u8; 32];
        k.iter_mut().zip(key).for_each(|(a, b)| *a = *b);
        Aes::A256(aes::Aes256::new(&k.into()))
    }

    fn encrypt_block(&self, block: &mut [u8; BLOCK]) {
        let b = aes::Block::cast_from_core_mut(block);
        match self {
            Aes::A128(c) => c.encrypt_block(b),
            Aes::A192(c) => c.encrypt_block(b),
            Aes::A256(c) => c.encrypt_block(b),
        }
    }

    /// CBC encryption of `data`, first zero-padded to whole blocks.
    fn encrypt_cbc(&self, iv: &[u8], data: &mut Vec<u8>) {
        data.resize(data.len().div_ceil(BLOCK) * BLOCK, 0);
        let mut prev = [0u8; BLOCK];
        prev.iter_mut().zip(iv).for_each(|(p, i)| *p = *i);
        for b in data.as_chunks_mut::<BLOCK>().0 {
            b.iter_mut().zip(prev).for_each(|(x, p)| *x ^= p);
            self.encrypt_block(b);
            prev = *b;
        }
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

/// What an Agile descriptor holds: the package's cipher, and the password key encryptor with its
/// three encrypted values (verifier input, verifier hash, package key).
struct AgileDescriptor {
    key_data: Params,
    key: Params,
    spin: u32,
    verifier_input: Vec<u8>,
    verifier_hash: Vec<u8>,
    key_value: Vec<u8>,
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
    Ok(AgileDescriptor {
        key_data,
        key,
        spin,
        verifier_input: field("encryptedVerifierHashInput")?,
        verifier_hash: field("encryptedVerifierHashValue")?,
        key_value: field("encryptedKeyValue")?,
    })
}

fn agile(xml: &[u8], package: &[u8], password: &str) -> Result<Vec<u8>, DecryptError> {
    let AgileDescriptor { key_data, key, spin, verifier_input, verifier_hash, key_value } = agile_descriptor(xml)?;
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

/// The random values one encryption draws.
pub(crate) struct Secrets {
    pub(crate) key_data_salt: [u8; 16],
    pub(crate) password_salt: [u8; 16],
    /// The package key.
    pub(crate) key: [u8; 32],
    pub(crate) verifier: [u8; 16],
    pub(crate) hmac_key: [u8; 64],
}

impl Secrets {
    pub(crate) fn random() -> Result<Secrets, EncryptError> {
        let mut s = Secrets { key_data_salt: [0; 16], password_salt: [0; 16], key: [0; 32], verifier: [0; 16], hmac_key: [0; 64] };
        for buf in [&mut s.key_data_salt[..], &mut s.password_salt, &mut s.key, &mut s.verifier, &mut s.hmac_key] {
            getrandom::fill(buf).map_err(|e| EncryptError::Random(e.to_string()))?;
        }
        Ok(s)
    }
}

/// `EncryptionInfo` and `EncryptedPackage` for `plain` with Agile encryption, AES-256 and
/// SHA-512 ([MS-OFFCRYPTO] 2.3.4.10 to 2.3.4.15).
pub(crate) fn encrypt_streams(plain: &[u8], password: &str, spin: u32, s: &Secrets) -> Result<(Vec<u8>, Vec<u8>), EncryptError> {
    let too_large = || EncryptError::TooLarge(format!("{} bytes", plain.len()));
    let aes = Aes::aes256(&s.key);
    let key_data = Params { hash: Hash::Sha512, salt: s.key_data_salt.to_vec(), key_len: s.key.len() };
    let mut package = u64::try_from(plain.len()).map_err(|_| too_large())?.to_le_bytes().to_vec();
    for (i, segment) in plain.chunks(SEGMENT).enumerate() {
        let index = u32::try_from(i).map_err(|_| too_large())?;
        let mut block = segment.to_vec();
        aes.encrypt_cbc(&key_data.iv(Some(&index.to_le_bytes())), &mut block);
        package.extend(block);
    }
    // The HMAC of the whole stream, size field included; its key and value sealed with the
    // package key.
    let seal = |data: &[u8], block_key: &[u8]| {
        let mut v = data.to_vec();
        aes.encrypt_cbc(&key_data.iv(Some(block_key)), &mut v);
        base64_encode(&v)
    };
    let hmac_key = seal(&s.hmac_key, &BLOCK_HMAC_KEY);
    let hmac_value = seal(&hmac_sha512(&s.hmac_key, &package), &BLOCK_HMAC_VALUE);
    // The password key encryptor: a verifier and the package key, under keys from the password.
    let key = Params { hash: Hash::Sha512, salt: s.password_salt.to_vec(), key_len: s.key.len() };
    let h = iterated_hash(key.hash, password, &key.salt, spin);
    let lock = |data: &[u8], block_key: &[u8]| {
        let mut v = data.to_vec();
        Aes::aes256(&derive(key.hash, &h, block_key, key.key_len)).encrypt_cbc(&key.iv(None), &mut v);
        base64_encode(&v)
    };
    let input = lock(&s.verifier, &BLOCK_VERIFIER_INPUT);
    let value = lock(&key.hash.digest(&[&s.verifier]), &BLOCK_VERIFIER_HASH);
    let secret = lock(&s.key, &BLOCK_KEY);
    let params = |salt: &[u8]| {
        format!(
            "saltSize=\"{}\" blockSize=\"16\" keyBits=\"256\" hashSize=\"64\" cipherAlgorithm=\"AES\" cipherChaining=\"ChainingModeCBC\" hashAlgorithm=\"SHA512\" saltValue=\"{}\"",
            salt.len(),
            base64_encode(salt)
        )
    };
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<encryption xmlns=\"http://schemas.microsoft.com/office/2006/encryption\" xmlns:p=\"{PASSWORD_ENCRYPTOR}\"><keyData {}/><dataIntegrity encryptedHmacKey=\"{hmac_key}\" encryptedHmacValue=\"{hmac_value}\"/><keyEncryptors><keyEncryptor uri=\"{PASSWORD_ENCRYPTOR}\"><p:encryptedKey spinCount=\"{spin}\" {} encryptedVerifierHashInput=\"{input}\" encryptedVerifierHashValue=\"{value}\" encryptedKeyValue=\"{secret}\"/></keyEncryptor></keyEncryptors></encryption>",
        params(&s.key_data_salt),
        params(&s.password_salt),
    );
    // Version 4.4 (Agile), then the reserved 0x40.
    let mut info = vec![4, 0, 4, 0, 0x40, 0, 0, 0];
    info.extend_from_slice(xml.as_bytes());
    Ok((info, package))
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

/// Standard base64, padded.
fn base64_encode(b: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let byte = |i: usize| u32::from(c.get(i).copied().unwrap_or(0));
        let n = (byte(0) << 16) | (byte(1) << 8) | byte(2);
        for i in 0..4 {
            let digit = ALPHABET.get((n >> (18 - 6 * i)) as usize & 63).copied().unwrap_or(b'A');
            s.push(if i <= c.len() { char::from(digit) } else { '=' });
        }
    }
    s
}

#[cfg(test)]
mod tests;
