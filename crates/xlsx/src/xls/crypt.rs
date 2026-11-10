//! Password-encrypted `.xls` workbooks ([MS-XLS] 2.2.10, [MS-OFFCRYPTO] 2.3.5, 2.3.6). A
//! FilePass record in the workbook globals holds a salt and a password verifier; the record data
//! after it is encrypted with RC4, rekeyed every 1024 bytes of the stream. Record headers, and the
//! data of a few records, are left in the clear.
//!
//! Two schemes are read: RC4 (version 1.1), keyed from the password through MD5, and RC4
//! CryptoAPI (versions 2.2, 3.2 and 4.2, what current Excel writes for an `.xls` saved with a
//! password), keyed through SHA-1 with a 40- to 128-bit key. XOR obfuscation, and CryptoAPI with
//! another cipher or hash, are reported as unsupported. Clean-room from Microsoft's public
//! specifications.

use md5::{Digest, Md5};
use sha1::Sha1;

use crate::crypto::DecryptError;

/// FilePass: the globals record that marks an encrypted workbook.
pub(super) const FILEPASS: u16 = 0x002F;
const EOF: u16 = 0x000A;
const BOUNDSHEET: u16 = 0x0085;
/// Records whose data is never encrypted: BOF, FilePass, UsrExcl, FileLock, InterfaceHdr,
/// RRDInfo and RRDHead.
const CLEAR: [u16; 7] = [0x0809, FILEPASS, 0x0194, 0x0195, 0x00E1, 0x0196, 0x0138];
/// The RC4 key changes every this many bytes of the stream.
const REKEY: usize = 1024;
/// CryptoAPI `AlgID` for RC4 ([MS-OFFCRYPTO] 2.3.2); 0 means the flags decide, which without
/// `fAES` is RC4 too.
const CALG_RC4: u32 = 0x6801;
/// CryptoAPI `AlgIDHash` for SHA-1; 0 means SHA-1 too.
const CALG_SHA1: u32 = 0x8004;
/// EncryptionHeader flags: `fExternal` (an external provider does the encryption) and `fAES`.
const F_EXTERNAL: u32 = 0x10;
const F_AES: u32 = 0x20;
/// A CryptoAPI salt is 16 bytes, the verifier too, and the verifier's SHA-1 hash 20.
const SALT_LEN: usize = 16;
const SHA1_LEN: usize = 20;

fn malformed(m: &str) -> DecryptError {
    DecryptError::Malformed(m.into())
}

/// The record header at `pos`: its type and data length.
fn header(stream: &[u8], pos: usize) -> Option<(u16, usize)> {
    let h = stream.get(pos..pos.checked_add(4)?)?;
    let id = u16::from_le_bytes([*h.first()?, *h.get(1)?]);
    Some((id, usize::from(u16::from_le_bytes([*h.get(2)?, *h.get(3)?]))))
}

/// The FilePass record's data, when the workbook globals hold one.
pub(super) fn filepass(stream: &[u8]) -> Option<&[u8]> {
    let mut pos = 0usize;
    while let Some((id, len)) = header(stream, pos) {
        let start = pos + 4;
        match id {
            EOF => return None,
            FILEPASS => return stream.get(start..start.saturating_add(len)),
            _ => pos = start + len,
        }
    }
    None
}

/// Decrypts a Workbook stream in place with `password`. The FilePass record stays where it is
/// (removing it would move the substreams the BoundSheet records point at).
pub(super) fn decrypt(stream: &mut [u8], password: &str) -> Result<(), DecryptError> {
    let key = password_key(filepass(stream).ok_or_else(|| malformed("no FilePass record"))?, password)?;
    let mut keystream = Keystream::new(key);
    let mut pos = 0usize;
    while let Some((id, len)) = header(stream, pos) {
        let start = pos + 4;
        let end = start.saturating_add(len).min(stream.len());
        // BoundSheet8's stream position (its first 4 bytes) stays in the clear.
        let clear = if CLEAR.contains(&id) {
            len
        } else if id == BOUNDSHEET {
            4
        } else {
            0
        };
        let from = start.saturating_add(clear).min(end);
        if let Some(data) = stream.get_mut(from..end) {
            keystream.apply(from, data);
        }
        pos = start + len;
    }
    Ok(())
}

/// What the block keys are made from.
enum Key {
    /// RC4 1.1: the 5 bytes every block key is the MD5 of, with the block number.
    Rc4([u8; 5]),
    /// RC4 CryptoAPI: H0, which every block key is the SHA-1 of, with the block number, cut to
    /// `bits`.
    CryptoApi { h0: [u8; SHA1_LEN], bits: u32 },
}

impl Key {
    /// The cipher for 1024-byte block `block`, at its start.
    fn block(&self, block: usize) -> Rc4 {
        let block = u32::try_from(block).unwrap_or(u32::MAX).to_le_bytes();
        match self {
            // MD5(key ‖ LE32(block)) ([MS-OFFCRYPTO] 2.3.6.2).
            Key::Rc4(key) => Rc4::new(&md5(&[key, &block])),
            // SHA-1(H0 ‖ LE32(block)), its first bits / 8 bytes; a 40-bit key is padded with
            // zeros to 128 bits ([MS-OFFCRYPTO] 2.3.5.2).
            Key::CryptoApi { h0, bits } => {
                let hash = sha1(&[h0, &block]);
                let mut key = [0u8; 16];
                let len = usize::try_from(*bits / 8).unwrap_or(0).min(key.len());
                key.iter_mut().zip(hash.iter()).take(len).for_each(|(k, h)| *k = *h);
                Rc4::new(if *bits == 40 { &key } else { key.get(..len).unwrap_or(&key) })
            }
        }
    }
}

/// The key every block key is derived from, after checking `password` against the FilePass
/// verifier.
fn password_key(filepass: &[u8], password: &str) -> Result<Key, DecryptError> {
    let short = || malformed("truncated FilePass record");
    match u16_at(filepass, 0).ok_or_else(short)? {
        0 => return Err(DecryptError::Unsupported("XOR obfuscation".into())),
        1 => {}
        other => return Err(DecryptError::Malformed(format!("unknown encryption type {other}"))),
    }
    match (u16_at(filepass, 2).ok_or_else(short)?, u16_at(filepass, 4).ok_or_else(short)?) {
        (1, 1) => rc4_key(filepass, password),
        (2..=4, 2) => cryptoapi_key(filepass, password),
        (major, minor) => Err(DecryptError::Unsupported(format!("version {major}.{minor}"))),
    }
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    let v = b.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([*v.first()?, *v.get(1)?]))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    let v = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([*v.first()?, *v.get(1)?, *v.get(2)?, *v.get(3)?]))
}

/// RC4 1.1 ([MS-OFFCRYPTO] 2.3.6.1, 2.3.6.2): the key, after checking the verifier.
fn rc4_key(filepass: &[u8], password: &str) -> Result<Key, DecryptError> {
    let short = || malformed("truncated FilePass record");
    let salt = filepass.get(6..22).ok_or_else(short)?;
    let verifier = filepass.get(22..38).ok_or_else(short)?;
    let verifier_hash = filepass.get(38..54).ok_or_else(short)?;

    // H0 = MD5(UTF-16LE password); H1 = MD5(16 × (first 5 bytes of H0 ‖ salt)); the key is the
    // first 5 bytes of H1.
    let pw: Vec<u8> = password.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let h0 = md5(&[&pw]);
    let truncated = h0.get(..5).unwrap_or_default();
    let mut m = Md5::new();
    for _ in 0..16 {
        m.update(truncated);
        m.update(salt);
    }
    let mut key = [0u8; 5];
    key.iter_mut().zip(m.finalize().iter()).for_each(|(k, h)| *k = *h);
    let key = Key::Rc4(key);

    // The verifier and its hash are encrypted one after the other with the block-0 key.
    let mut rc4 = key.block(0);
    let verifier: Vec<u8> = verifier.iter().map(|b| b ^ rc4.next_byte()).collect();
    let verifier_hash: Vec<u8> = verifier_hash.iter().map(|b| b ^ rc4.next_byte()).collect();
    if md5(&[&verifier]).as_slice() != verifier_hash.as_slice() {
        return Err(DecryptError::WrongPassword);
    }
    Ok(key)
}

/// RC4 CryptoAPI ([MS-OFFCRYPTO] 2.3.5.1, 2.3.5.2, 2.3.5.6): after the version, the flags, the
/// EncryptionHeader's size and the header itself, then the EncryptionVerifier (salt size, salt,
/// encrypted verifier, verifier hash size, encrypted verifier hash). Every size is the file's
/// word, so each is checked before it is used.
fn cryptoapi_key(filepass: &[u8], password: &str) -> Result<Key, DecryptError> {
    let short = || malformed("truncated FilePass record");
    let unsupported = |scheme: String| Err(DecryptError::Unsupported(scheme));
    let flags = u32_at(filepass, 6).ok_or_else(short)?;
    let header_size = usize::try_from(u32_at(filepass, 10).ok_or_else(short)?).unwrap_or(usize::MAX);
    let header_end = 14usize.saturating_add(header_size);
    let header = filepass.get(14..header_end).ok_or_else(short)?;
    // Eight words (Flags, SizeExtra, AlgID, AlgIDHash, KeySize, ProviderType and two reserved),
    // then the provider's name, which says nothing the reader needs.
    let (true, Some(alg), Some(alg_hash), Some(bits)) = (header.len() >= 32, u32_at(header, 8), u32_at(header, 12), u32_at(header, 16)) else {
        return Err(malformed("EncryptionHeader too short"));
    };
    if flags & F_EXTERNAL != 0 {
        return unsupported("CryptoAPI with an external provider".into());
    }
    if flags & F_AES != 0 {
        return unsupported("CryptoAPI AES".into());
    }
    if !matches!(alg, 0 | CALG_RC4) {
        return unsupported(format!("CryptoAPI with cipher 0x{alg:04X}"));
    }
    if !matches!(alg_hash, 0 | CALG_SHA1) {
        return unsupported(format!("RC4 CryptoAPI with hash 0x{alg_hash:04X}"));
    }
    // 0 means 40 bits; otherwise 40 to 128 bits in steps of 8.
    let bits = if bits == 0 { 40 } else { bits };
    if !(40..=128).contains(&bits) || bits % 8 != 0 {
        return unsupported(format!("RC4 CryptoAPI with a {bits}-bit key"));
    }

    let verifier_at = header_end;
    let field = |at: usize, len: usize| filepass.get(verifier_at.saturating_add(at)..verifier_at.saturating_add(at + len));
    if u32_at(filepass, verifier_at) != Some(SALT_LEN as u32) {
        return Err(field(0, 4).map_or_else(short, |_| malformed("unexpected salt size")));
    }
    let salt = field(4, SALT_LEN).ok_or_else(short)?;
    let verifier = field(4 + SALT_LEN, SALT_LEN).ok_or_else(short)?;
    if u32_at(filepass, verifier_at.saturating_add(4 + 2 * SALT_LEN)) != Some(SHA1_LEN as u32) {
        return Err(field(4 + 2 * SALT_LEN, 4).map_or_else(short, |_| malformed("unexpected verifier hash size")));
    }
    let verifier_hash = field(8 + 2 * SALT_LEN, SHA1_LEN).ok_or_else(short)?;

    // H0 = SHA-1(salt ‖ UTF-16LE password).
    let pw: Vec<u8> = password.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let key = Key::CryptoApi { h0: sha1(&[salt, &pw]), bits };

    // As with RC4 1.1, the verifier and its hash run on through one block-0 cipher.
    let mut rc4 = key.block(0);
    let verifier: Vec<u8> = verifier.iter().map(|b| b ^ rc4.next_byte()).collect();
    let verifier_hash: Vec<u8> = verifier_hash.iter().map(|b| b ^ rc4.next_byte()).collect();
    if sha1(&[&verifier]).as_slice() != verifier_hash.as_slice() {
        return Err(DecryptError::WrongPassword);
    }
    Ok(key)
}

fn md5(parts: &[&[u8]]) -> [u8; 16] {
    let mut m = Md5::new();
    for p in parts {
        m.update(p);
    }
    let mut out = [0u8; 16];
    out.iter_mut().zip(m.finalize().iter()).for_each(|(o, h)| *o = *h);
    out
}

fn sha1(parts: &[&[u8]]) -> [u8; SHA1_LEN] {
    let mut m = Sha1::new();
    for p in parts {
        m.update(p);
    }
    let mut out = [0u8; SHA1_LEN];
    out.iter_mut().zip(m.finalize().iter()).for_each(|(o, h)| *o = *h);
    out
}

/// The keystream by stream offset: byte `n` comes from block `n / 1024`'s key, at position
/// `n % 1024`, whether or not the bytes before it were encrypted.
struct Keystream {
    key: Key,
    block: usize,
    rc4: Rc4,
    /// The stream offset of the next byte `rc4` gives.
    at: usize,
}

impl Keystream {
    fn new(key: Key) -> Keystream {
        Keystream { rc4: key.block(0), key, block: 0, at: 0 }
    }

    /// XORs `data`, found at stream offset `offset`, with the keystream there. Offsets only
    /// move forward between calls in the normal case; going back rekeys.
    fn apply(&mut self, offset: usize, data: &mut [u8]) {
        for (n, b) in data.iter_mut().enumerate() {
            *b ^= self.byte_at(offset.saturating_add(n));
        }
    }

    fn byte_at(&mut self, offset: usize) -> u8 {
        let block = offset / REKEY;
        if block != self.block || offset < self.at {
            self.block = block;
            self.rc4 = self.key.block(block);
            self.at = block * REKEY;
        }
        while self.at < offset {
            self.rc4.next_byte();
            self.at += 1;
        }
        self.at += 1;
        self.rc4.next_byte()
    }
}

/// The RC4 stream cipher.
struct Rc4 {
    s: [u8; 256],
    i: u8,
    j: u8,
}

impl Rc4 {
    /// RC4 keyed with `key` (5 to 16 bytes here; an empty key acts as one zero byte).
    fn new(key: &[u8]) -> Rc4 {
        let mut s = [0u8; 256];
        for (n, x) in (0u8..=255).zip(s.iter_mut()) {
            *x = n;
        }
        let mut j = 0u8;
        for n in 0..256 {
            let k = key.get(n % key.len().max(1)).copied().unwrap_or(0);
            j = j.wrapping_add(s[n]).wrapping_add(k);
            s.swap(n, usize::from(j));
        }
        Rc4 { s, i: 0, j: 0 }
    }

    fn next_byte(&mut self) -> u8 {
        self.i = self.i.wrapping_add(1);
        self.j = self.j.wrapping_add(self.s[usize::from(self.i)]);
        self.s.swap(usize::from(self.i), usize::from(self.j));
        let t = self.s[usize::from(self.i)].wrapping_add(self.s[usize::from(self.j)]);
        self.s[usize::from(t)]
    }
}

/// Encrypts a Workbook stream the way [`decrypt`] reads it, for tests: the FilePass record
/// (RC4 1.1 with a fixed salt and verifier) goes right after the globals' BOF. The stream must
/// start with that BOF.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn encrypt(stream: &[u8], password: &str) -> Vec<u8> {
    let salt: [u8; 16] = std::array::from_fn(|n| n as u8 + 0x40);
    let verifier: [u8; 16] = std::array::from_fn(|n| n as u8 + 0x60);
    let pw: Vec<u8> = password.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let h0 = md5(&[&pw]);
    let mut m = Md5::new();
    for _ in 0..16 {
        m.update(&h0[..5]);
        m.update(salt);
    }
    let mut key = [0u8; 5];
    key.copy_from_slice(&m.finalize()[..5]);
    let mut rc4 = Key::Rc4(key).block(0);
    let mut filepass = vec![1, 0, 1, 0, 1, 0];
    filepass.extend(salt);
    filepass.extend(verifier.iter().map(|b| b ^ rc4.next_byte()));
    filepass.extend(md5(&[&verifier]).iter().map(|b| b ^ rc4.next_byte()));
    with_filepass(stream, &filepass, password)
}

/// The FilePass data [`encrypt_cryptoapi`] writes: RC4 CryptoAPI version 4.2, as current Excel
/// writes it, with a `bits`-bit key (40 to 128) and a fixed salt and verifier.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn cryptoapi_filepass(password: &str, bits: u32) -> Vec<u8> {
    let salt: [u8; SALT_LEN] = std::array::from_fn(|n| (n as u8).wrapping_mul(7).wrapping_add(0x11));
    let verifier: [u8; SALT_LEN] = std::array::from_fn(|n| n as u8 + 0x80);
    let pw: Vec<u8> = password.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut rc4 = Key::CryptoApi { h0: sha1(&[&salt, &pw]), bits }.block(0);
    // fCryptoAPI and fDocProps, as Excel sets them.
    let flags: u32 = 0x0C;
    let provider = "Test provider\0".encode_utf16().flat_map(u16::to_le_bytes);
    let header: Vec<u8> = [flags, 0, CALG_RC4, CALG_SHA1, bits, 1, 0, 0].iter().flat_map(|w| w.to_le_bytes()).chain(provider).collect();
    let mut filepass = vec![1, 0, 4, 0, 2, 0];
    filepass.extend(flags.to_le_bytes());
    filepass.extend((header.len() as u32).to_le_bytes());
    filepass.extend(header);
    filepass.extend((SALT_LEN as u32).to_le_bytes());
    filepass.extend(salt);
    filepass.extend(verifier.iter().map(|b| b ^ rc4.next_byte()));
    filepass.extend((SHA1_LEN as u32).to_le_bytes());
    filepass.extend(sha1(&[&verifier]).iter().map(|b| b ^ rc4.next_byte()));
    filepass
}

/// [`encrypt`] with RC4 CryptoAPI ([`cryptoapi_filepass`]).
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn encrypt_cryptoapi(stream: &[u8], password: &str, bits: u32) -> Vec<u8> {
    with_filepass(stream, &cryptoapi_filepass(password, bits), password)
}

/// `stream` with a FilePass record holding `filepass` right after its first BOF, encrypted with
/// `password`.
#[cfg(any(test, feature = "test-support"))]
#[allow(clippy::unwrap_used)]
fn with_filepass(stream: &[u8], filepass: &[u8], password: &str) -> Vec<u8> {
    let bof_len = 4 + usize::from(u16::from_le_bytes([stream[2], stream[3]]));
    let mut out = stream[..bof_len].to_vec();
    out.extend(FILEPASS.to_le_bytes());
    out.extend((filepass.len() as u16).to_le_bytes());
    out.extend(filepass);
    let shift = out.len() - bof_len;
    let mut rest = stream[bof_len..].to_vec();
    // BoundSheet8 positions move by the FilePass record's size.
    let mut pos = 0;
    while let Some((id, len)) = header(&rest, pos) {
        if id == BOUNDSHEET {
            let at = pos + 4;
            let p = u32::from_le_bytes(rest[at..at + 4].try_into().unwrap()) + shift as u32;
            rest[at..at + 4].copy_from_slice(&p.to_le_bytes());
        }
        pos += 4 + len;
    }
    out.extend(rest);
    // The cipher is its own inverse.
    decrypt(&mut out, password).unwrap();
    out
}
