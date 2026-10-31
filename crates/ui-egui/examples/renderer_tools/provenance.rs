//! Shared by the native example and its xtask build/run guard; no renderer dependency.
use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const MAX_FILES: usize = 20_000;
const MAX_SOURCE_BYTES: u64 = 256 * 1024 * 1024;

pub struct Source {
    pub head: String,
    pub digest: String,
    pub manifest: Value,
}

pub fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("git: {e}"))?;
    let mut bytes = Vec::new();
    child.stdout.take().ok_or("git stdout unavailable")?.take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() > 16 * 1024 * 1024 {
        let _ = child.kill();
        let _ = child.wait();
        return Err("git output exceeds 16 MiB".into());
    }
    if !child.wait().map_err(|e| e.to_string())?.success() {
        return Err(format!("git {} failed", args.join(" ")));
    }
    Ok(bytes)
}

pub fn bytes_hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

pub fn file_hash(path: &Path, limit: u64) -> Result<String, String> {
    let file = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if file.metadata().map_err(|e| e.to_string())?.len() > limit {
        return Err(format!("{} exceeds the {limit}-byte input limit", path.display()));
    }
    let mut reader = file.take(limit.saturating_add(1));
    let mut hash = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 16_384];
    loop {
        let len = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if len == 0 {
            break;
        }
        total = total.saturating_add(len as u64);
        if total > limit {
            return Err("input grew past its size limit".into());
        }
        hash.update(buffer.get(..len).ok_or("invalid read length")?);
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Inventory actual checkout bytes, including nonignored untracked inputs and deletions.
/// Refuse symlinks rather than pretending to identify build inputs outside the checkout.
pub fn capture(root: &Path) -> Result<Source, String> {
    let paths = git(root, &["ls-files", "-z", "--cached", "--others", "--exclude-standard"])?;
    let mut unique = BTreeSet::new();
    for path in paths.split(|b| *b == 0).filter(|s| !s.is_empty()) {
        unique.insert(path);
        if unique.len() > MAX_FILES {
            return Err("source inventory exceeds 20,000 files".into());
        }
    }
    let paths = unique;
    let mut records = Vec::with_capacity(paths.len());
    let mut total = 0u64;
    for raw in paths {
        let relative = std::str::from_utf8(raw).map_err(|_| "source paths must be UTF-8")?;
        if !Path::new(relative).components().all(|c| matches!(c, std::path::Component::Normal(_))) {
            return Err("nonrelative source path".into());
        }
        let path = root.join(relative);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                records.push(json!({"path": relative, "deleted": true}));
                continue;
            }
            Err(e) => return Err(format!("{relative}: {e}")),
        };
        if !metadata.is_file() {
            return Err(format!("source input {relative} must be a regular file (no symlinks/submodules)"));
        }
        total = total.checked_add(metadata.len()).ok_or("source inventory size overflow")?;
        if total > MAX_SOURCE_BYTES {
            return Err("source inventory exceeds 256 MiB".into());
        }
        records.push(json!({"path": relative, "bytes": metadata.len(), "sha256": file_hash(&path, 32 * 1024 * 1024)?}));
    }
    let head = String::from_utf8(git(root, &["rev-parse", "HEAD"])?).map_err(|e| e.to_string())?.trim().to_owned();
    let digest = inventory_digest(&records)?;
    let dirty = !git(root, &["status", "--porcelain=v1", "--untracked-files=normal"])?.is_empty();
    let manifest = json!({"head": head, "sha256": digest, "dirty": dirty, "files": records});
    Ok(Source { head, digest, manifest })
}

fn inventory_digest(records: &[Value]) -> Result<String, String> {
    // Fixed tuples keep the digest independent of serde_json's preserve_order feature,
    // which can differ between the xtask and renderer dependency graphs.
    let ordered: Vec<_> = records.iter().map(|r| (r["path"].as_str(), r["bytes"].as_u64(), r["sha256"].as_str())).collect();
    Ok(bytes_hash(&serde_json::to_vec(&ordered).map_err(|e| e.to_string())?))
}

pub fn unchanged(before: &Source, after: &Source) -> Result<(), String> {
    if before.head != after.head || before.digest != after.digest {
        return Err("source changed during build/run; discard this measurement and rerun".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(head: &str, digest: &str) -> Source {
        Source { head: head.into(), digest: digest.into(), manifest: Value::Null }
    }

    #[test]
    fn refuses_changed_head_even_with_identical_files() {
        assert!(unchanged(&source("a", "same"), &source("b", "same")).is_err());
    }

    #[test]
    fn refuses_changed_checkout_bytes_at_the_same_head() {
        assert!(unchanged(&source("a", "old"), &source("a", "new")).is_err());
        assert!(unchanged(&source("a", "same"), &source("a", "same")).is_ok());
    }

    #[test]
    fn inventory_wire_format_is_independent_of_object_key_order() {
        let record = json!({"sha256":"digest","bytes":3,"path":"a"});
        assert_eq!(inventory_digest(&[record]).unwrap(), bytes_hash(br#"[["a",3,"digest"]]"#));
        let deleted = json!({"path":"a","deleted":true});
        let empty = json!({"path":"a","bytes":0,"sha256":"empty"});
        assert_ne!(inventory_digest(&[deleted]).unwrap(), inventory_digest(&[empty]).unwrap());
    }
}
