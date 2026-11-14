//! Excel-compatible password hashes for protection (ECMA-376 Part 1 §18.2.29 and §18.3.1.66
//! attribute sets): the legacy 16-bit hash (`password="CC1A"`) and the iterated, salted hash
//! (`algorithmName`, `hashValue`, `saltValue`, `spinCount`); and the unsalted SHA-256 digest
//! OpenDocument stores (`table:protection-key`). A sheet or workbook password set in GridCraft
//! keeps both an Excel form and the SHA-256 one, so it survives saving in either format (edit
//! ranges keep only the Excel form). Protection is a convenience, not security, but verification
//! really checks the password: an unsupported variant is refused, never accepted.

use serde::{Deserialize, Serialize};
use sha2::Digest;

/// Excel's default spin count for new passwords.
pub const DEFAULT_SPIN_COUNT: u32 = 100_000;
/// Largest spin count accepted when verifying (the documented maximum); a file asking for more
/// would only make verification hang.
pub const MAX_SPIN_COUNT: u32 = 10_000_000;
/// Largest decoded hash or salt accepted from a file (a SHA-512 hash is 64 bytes, Excel's salts
/// 16); anything longer is damaged, and decoding it would only allocate.
const MAX_STORED_BYTES: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PasswordHash {
    /// The legacy 16-bit hash.
    Legacy(u16),
    /// An iterated, salted hash as stored in the file: algorithm name, base64 hash and salt.
    Iterated { algorithm: String, hash: String, salt: String, spin_count: u32 },
    /// The base64 SHA-256 digest of the password's UTF-8 bytes, unsalted: the form OpenDocument
    /// stores in `table:protection-key` (ODF 1.3 part 3 §19.701) when
    /// `table:protection-key-digest-algorithm` (§19.702) names SHA-256. A writer writes both
    /// attributes; a key under any other or no digest algorithm is not this form.
    Sha256Digest(String),
    /// A password set in GridCraft, kept in the form each file format stores: Excel's hash
    /// (which verifies it) and the SHA-256 digest.
    Both { excel: Box<PasswordHash>, odf: String },
}

impl PasswordHash {
    /// A SHA-512 hash with Excel's default spin count, for a new password.
    pub fn new_sha512(password: &str, salt: &[u8]) -> PasswordHash {
        let hash = iterated::<sha2::Sha512>(password, salt, DEFAULT_SPIN_COUNT);
        PasswordHash::Iterated {
            algorithm: "SHA-512".into(),
            hash: crate::b64::encode(&hash),
            salt: crate::b64::encode(salt),
            spin_count: DEFAULT_SPIN_COUNT,
        }
    }

    /// A SHA-256 digest of a new password (the form OpenDocument stores).
    pub fn new_sha256_digest(password: &str) -> PasswordHash {
        PasswordHash::Sha256Digest(sha256_digest(password))
    }

    /// A new password in every form a file keeps: Excel's (SHA-512, salted) and the SHA-256
    /// digest, so it survives saving in either format.
    pub fn new_for_files(password: &str, salt: &[u8]) -> PasswordHash {
        PasswordHash::Both { excel: Box::new(PasswordHash::new_sha512(password, salt)), odf: sha256_digest(password) }
    }

    /// The hash as Excel stores it, if it has that form. Not recursive: a `Both` read from a
    /// damaged file that nests another `Both` has no Excel form.
    pub fn excel(&self) -> Option<&PasswordHash> {
        match self {
            PasswordHash::Legacy(_) | PasswordHash::Iterated { .. } => Some(self),
            PasswordHash::Sha256Digest(_) => None,
            PasswordHash::Both { excel, .. } => match excel.as_ref() {
                h @ (PasswordHash::Legacy(_) | PasswordHash::Iterated { .. }) => Some(h),
                PasswordHash::Sha256Digest(_) | PasswordHash::Both { .. } => None,
            },
        }
    }

    /// The SHA-256 digest (base64), if it has that form.
    pub fn odf_key(&self) -> Option<&str> {
        match self {
            PasswordHash::Sha256Digest(k) | PasswordHash::Both { odf: k, .. } => Some(k),
            PasswordHash::Legacy(_) | PasswordHash::Iterated { .. } => None,
        }
    }

    /// Whether `password` matches. `Err` when the hash can't be checked (unsupported algorithm,
    /// broken base64, spin count over [`MAX_SPIN_COUNT`]).
    pub fn verify(&self, password: &str) -> Result<bool, String> {
        match self {
            PasswordHash::Legacy(h) => Ok(legacy_hash(password) == *h),
            // Checked through its Excel form (one level only: see `excel`), else its digest.
            PasswordHash::Both { odf, .. } => match self.excel() {
                Some(excel) => excel.verify(password),
                None => verify_sha256_digest(odf, password),
            },
            PasswordHash::Sha256Digest(hash) => verify_sha256_digest(hash, password),
            PasswordHash::Iterated { algorithm, hash, salt, spin_count } => {
                if algorithm.is_empty() {
                    return Err("the stored password hash is damaged".into());
                }
                if *spin_count > MAX_SPIN_COUNT {
                    return Err(format!("the password uses a spin count of {spin_count}, more than the {MAX_SPIN_COUNT} allowed"));
                }
                let (Some(want), Some(salt)) =
                    (crate::b64::decode_limited(hash, MAX_STORED_BYTES), crate::b64::decode_limited(salt, MAX_STORED_BYTES))
                else {
                    return Err("the stored password hash is damaged".into());
                };
                let got = match algorithm.to_ascii_uppercase().replace('_', "-").as_str() {
                    "SHA-512" => iterated::<sha2::Sha512>(password, &salt, *spin_count),
                    "SHA-384" => iterated::<sha2::Sha384>(password, &salt, *spin_count),
                    "SHA-256" => iterated::<sha2::Sha256>(password, &salt, *spin_count),
                    _ => return Err(format!("passwords hashed with {algorithm} aren't supported")),
                };
                Ok(got == want)
            }
        }
    }
}

/// The base64 SHA-256 digest of `password`'s UTF-8 bytes.
fn sha256_digest(password: &str) -> String {
    crate::b64::encode(&sha2::Sha256::digest(password.as_bytes()))
}

fn verify_sha256_digest(hash: &str, password: &str) -> Result<bool, String> {
    let want = crate::b64::decode_limited(hash, 64).ok_or_else(|| "the stored password hash is damaged".to_string())?;
    Ok(sha2::Sha256::digest(password.as_bytes()).as_slice() == want.as_slice())
}

/// A 16-byte salt derived from `seed` (callers mix in what varies: names, a counter, the clock).
pub fn salt_from(seed: &[u8]) -> [u8; 16] {
    let h = sha2::Sha512::digest(seed);
    let mut out = [0u8; 16];
    for (o, b) in out.iter_mut().zip(h.iter()) {
        *o = *b;
    }
    out
}

/// The legacy 16-bit password hash: each UTF-16 unit contributes its low byte (its high byte
/// when the low one is zero), folded in from the last character with a 15-bit rotation, then
/// the length and the constant `0xCE4B`.
pub fn legacy_hash(password: &str) -> u16 {
    let bytes: Vec<u16> = password.encode_utf16().map(|u| if u & 0xFF != 0 { u & 0xFF } else { u >> 8 }).collect();
    let rotate = |v: u16| ((v >> 14) & 1) | ((v << 1) & 0x7FFF);
    let mut h: u16 = 0;
    for b in bytes.iter().rev() {
        h = rotate(h) ^ b;
    }
    h = rotate(h) ^ (bytes.len() as u16);
    h ^ 0xCE4B
}

/// H0 = H(salt + UTF-16LE password); Hn = H(Hn-1 + n as 32-bit little-endian), `spin` times.
fn iterated<D: Digest>(password: &str, salt: &[u8], spin: u32) -> Vec<u8> {
    let mut d = D::new();
    d.update(salt);
    for u in password.encode_utf16() {
        d.update(u.to_le_bytes());
    }
    let mut h = d.finalize().to_vec();
    for i in 0..spin {
        let mut d = D::new();
        d.update(&h);
        d.update(i.to_le_bytes());
        h = d.finalize().to_vec();
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_hash_matches_the_published_example() -> Result<(), Box<dyn std::error::Error>> {
        // The hash Excel writes for the password "password" (`password="83AF"`).
        assert_eq!(legacy_hash("password"), 0x83AF);
        assert!(PasswordHash::Legacy(0x83AF).verify("password")?);
        assert!(!PasswordHash::Legacy(0x83AF).verify("Password")?);
        assert_eq!(legacy_hash(""), 0xCE4B);
        Ok(())
    }

    #[test]
    fn iterated_hash_verifies_and_refuses_what_it_cannot_check() -> Result<(), Box<dyn std::error::Error>> {
        let h = PasswordHash::new_sha512("s3cret", b"0123456789abcdef");
        assert!(h.verify("s3cret")?);
        assert!(!h.verify("s3cret ")?);
        assert!(matches!(&h, PasswordHash::Iterated { algorithm, spin_count: DEFAULT_SPIN_COUNT, .. } if algorithm == "SHA-512"));
        let PasswordHash::Iterated { hash, salt, .. } = h else { return Err("not an iterated hash".into()) };
        let other =
            |algorithm: &str, spin_count| PasswordHash::Iterated { algorithm: algorithm.into(), hash: hash.clone(), salt: salt.clone(), spin_count };
        assert!(other("SHA-1", 10).verify("s3cret").is_err());
        assert!(other("MD5", 10).verify("s3cret").is_err());
        assert!(other("SHA-512", MAX_SPIN_COUNT + 1).verify("s3cret").is_err());
        assert!(!other("SHA-256", 2).verify("s3cret")?);
        let broken = PasswordHash::Iterated { algorithm: "SHA-512".into(), hash: "!!".into(), salt: salt.clone(), spin_count: 1 };
        assert!(broken.verify("s3cret").is_err());
        let huge_salt = PasswordHash::Iterated { algorithm: "SHA-512".into(), hash, salt: "A".repeat(4 * MAX_STORED_BYTES), spin_count: 1 };
        assert!(huge_salt.verify("s3cret").is_err());
        Ok(())
    }

    #[test]
    fn sha256_digests_verify() -> Result<(), Box<dyn std::error::Error>> {
        // SHA-256 of "abc", base64.
        let h = PasswordHash::Sha256Digest("ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0=".into());
        assert_eq!(h, PasswordHash::new_sha256_digest("abc"));
        assert!(h.verify("abc")?);
        assert!(!h.verify("abd")?);
        assert!(PasswordHash::Sha256Digest("!!".into()).verify("abc").is_err());
        assert_eq!(h.excel(), None);
        assert_eq!(h.odf_key(), Some("ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0="));
        Ok(())
    }

    #[test]
    fn passwords_set_in_gridcraft_keep_both_forms() -> Result<(), Box<dyn std::error::Error>> {
        let h = PasswordHash::new_for_files("pa55", b"0123456789abcdef");
        assert_eq!(h.excel(), Some(&PasswordHash::new_sha512("pa55", b"0123456789abcdef")));
        assert_eq!(h.odf_key(), PasswordHash::new_sha256_digest("pa55").odf_key());
        assert!(h.verify("pa55")?);
        assert!(!h.verify("pass")?);
        // Each form checks the password on its own.
        assert!(h.excel().ok_or("no Excel form")?.verify("pa55")?);
        assert!(PasswordHash::Sha256Digest(h.odf_key().ok_or("no digest")?.into()).verify("pa55")?);
        // A nested `Both` (only a damaged file makes one) has no Excel form and falls back to its digest.
        let nested = PasswordHash::Both { excel: Box::new(h.clone()), odf: PasswordHash::new_sha256_digest("x").odf_key().unwrap_or("").into() };
        assert_eq!(nested.excel(), None);
        assert!(nested.verify("x")?);
        assert!(!nested.verify("pa55")?);
        // The JSON form round-trips.
        let back: PasswordHash = serde_json::from_str(&serde_json::to_string(&h)?)?;
        assert_eq!(back, h);
        Ok(())
    }
}
