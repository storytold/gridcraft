//! One-shot binding of an opaque editor frame to its embedding parent.

/// The bundle declares its identity from the last pinned deployment path segment.
pub fn bundle_from_path(path: &str) -> Option<&str> {
    let (_, tail) = path.rsplit_once("/b/")?;
    let bundle = tail.strip_suffix("/index.html")?;
    (bundle.len() == 64 && bundle.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))).then_some(bundle)
}

pub struct Handshake {
    pub parent_origin: String,
    pub bundle: String,
    announced: bool,
    connected: bool,
}

impl Handshake {
    pub fn new(parent_origin: String, bundle: String) -> Self {
        Self { parent_origin, bundle, announced: false, connected: false }
    }

    pub fn announced(&mut self) {
        self.announced = true;
    }

    pub fn accept(&mut self, parent: bool, origin: &str, kind: Option<&str>, protocol: Option<f64>, bundle: Option<&str>, ports: usize) -> bool {
        if !self.announced
            || self.connected
            || !parent
            || origin != self.parent_origin
            || kind != Some("gridcraft:connect")
            || protocol != Some(1.0)
            || bundle != Some(self.bundle.as_str())
            || ports != 1
        {
            return false;
        }
        self.connected = true;
        true
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn bundle_identity_is_the_last_pinned_bundle_path_segment() {
        let hash = "a".repeat(64);
        for prefix in ["", "/workbook-editor", "/b/old"] {
            assert_eq!(super::bundle_from_path(&format!("{prefix}/b/{hash}/index.html")), Some(hash.as_str()));
        }
        for path in ["/index.html", "/b/short/index.html", "/b/ABC/index.html", "/b/a/other/index.html"] {
            assert!(super::bundle_from_path(path).is_none());
        }
        assert!(super::bundle_from_path(&format!("/b/{}/index.html", "A".repeat(64))).is_none());
    }

    #[test]
    fn only_exact_parent_origin_bundle_protocol_and_one_port_bind_once_after_hello() {
        let hash = "a".repeat(64);
        let mut binding = super::Handshake::new("https://host.example".into(), hash.clone());
        assert!(!binding.accept(true, "https://host.example", Some("gridcraft:connect"), Some(1.0), Some(&hash), 1));
        binding.announced();
        for (parent, origin, kind, protocol, bundle, ports) in [
            (false, "https://host.example", Some("gridcraft:connect"), Some(1.0), Some(hash.as_str()), 1),
            (true, "https://other.example", Some("gridcraft:connect"), Some(1.0), Some(hash.as_str()), 1),
            (true, "https://host.example", Some("gridcraft:open"), Some(1.0), Some(hash.as_str()), 1),
            (true, "https://host.example", Some("gridcraft:connect"), Some(2.0), Some(hash.as_str()), 1),
            (true, "https://host.example", Some("gridcraft:connect"), Some(1.0), Some("wrong"), 1),
            (true, "https://host.example", Some("gridcraft:connect"), Some(1.0), Some(hash.as_str()), 0),
            (true, "https://host.example", Some("gridcraft:connect"), Some(1.0), Some(hash.as_str()), 2),
        ] {
            assert!(!binding.accept(parent, origin, kind, protocol, bundle, ports));
        }
        assert!(binding.accept(true, "https://host.example", Some("gridcraft:connect"), Some(1.0), Some(&hash), 1));
        assert!(!binding.accept(true, "https://host.example", Some("gridcraft:connect"), Some(1.0), Some(&hash), 1));
    }
}
