//! IMAGE: a picture from an https URL, shown in the cell.

use std::fmt::Debug;

use gridcraft_core::{CellError, PictureSizing, Value};

use crate::util::{R, has, int, text};
use crate::{Arg, Ctx};

/// Longest URL IMAGE accepts, in characters.
const MAX_URL: usize = 2048;
/// Largest picture IMAGE keeps, in bytes. A host's fetcher may return more; the function refuses it.
pub const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;

/// Why a picture could not be fetched, which decides the error IMAGE shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FetchError {
    /// The host does not allow this request (its settings or policy): `#BLOCKED!`.
    Blocked,
    /// The request was allowed but failed (unreachable host, server error, timeout): `#CONNECT!`.
    Connect,
}

impl From<FetchError> for CellError {
    fn from(e: FetchError) -> CellError {
        match e {
            FetchError::Blocked => CellError::Blocked,
            FetchError::Connect => CellError::Connect,
        }
    }
}

/// Fetches the pictures IMAGE asks for. The host installs one; without it every IMAGE is
/// `#BLOCKED!`.
///
/// `get` runs while formulas calculate, so it should return promptly (apply a timeout). By the time
/// it is called the URL is a well-formed `https` URL of at most 2048 characters. A host that makes
/// real requests decides what a workbook may reach (local addresses, redirects, credentials, size)
/// and answers [`FetchError::Blocked`] for what it refuses.
pub trait ImageFetcher: Debug + Send + Sync {
    /// The encoded picture at `url`, or why there is none.
    fn get(&self, url: &str) -> Result<Vec<u8>, FetchError>;
}

/// A picture a formula produced, for the host to show in the cell it is entered in.
#[derive(Clone, Debug, PartialEq)]
pub struct ImagePicture {
    /// The encoded picture: PNG, JPEG, GIF or BMP bytes.
    pub data: Vec<u8>,
    /// The media type matching `data`, for example `image/png`.
    pub mime: &'static str,
    /// The text IMAGE returns and screen readers read.
    pub alt: String,
    /// How the picture fills its cell, from IMAGE's sizing argument.
    pub sizing: PictureSizing,
}

/// The media type of an encoded picture, from its leading bytes. `None` for formats IMAGE does not
/// support.
pub fn sniff(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if data.starts_with(b"BM") {
        Some("image/bmp")
    } else {
        None
    }
}

/// Whether `url` is an `https://` URL with a host and no spaces or control characters.
fn is_https_url(url: &str) -> bool {
    let lower = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    lower == "https://"
        && url.get(8..).is_some_and(|rest| !rest.is_empty() && !rest.starts_with(['/', '?', '#']))
        && !url.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// IMAGE(source, [alt_text], [sizing], [height], [width]).
///
/// The picture comes from [`Ctx::image_get`]: `#BLOCKED!` when the host has no fetcher or refuses
/// the URL, `#CONNECT!` when the request fails, `#VALUE!` when the response is not a supported
/// picture or the arguments are invalid. Sizing follows Excel: 0 fits the cell keeping the aspect ratio,
/// 1 fills it, 2 keeps the original size, 3 uses `height` and `width` (required, in pixels).
pub(crate) fn image(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let source = text(a, 0)?;
    if source.chars().take(MAX_URL + 1).count() > MAX_URL || !is_https_url(&source) {
        return Err(CellError::Value);
    }
    let alt = if has(a, 1) { text(a, 1)? } else { String::new() };
    let code = if has(a, 2) { int(a, 2)? } else { 0 };
    let height = if has(a, 3) { Some(int(a, 3)?) } else { None };
    let width = if has(a, 4) { Some(int(a, 4)?) } else { None };
    let sizing = PictureSizing::from_code(code, height, width).ok_or(CellError::Value)?;
    let data = c.image_get(&source)?;
    if data.len() > MAX_IMAGE_BYTES {
        return Err(CellError::Value);
    }
    let mime = sniff(&data).ok_or(CellError::Value)?;
    c.set_image(ImagePicture { data, mime, alt: alt.clone(), sizing });
    Ok(Value::from(alt))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::testutil::{e, t};

    /// A context whose fetcher answers one URL; any other is unreachable.
    struct Fetch {
        bytes: Result<Vec<u8>, CellError>,
        set: std::cell::RefCell<Option<ImagePicture>>,
    }

    impl Ctx for Fetch {
        fn date_system(&self) -> gridcraft_core::DateSystem {
            gridcraft_core::DateSystem::D1900
        }
        fn now_serial(&self) -> f64 {
            0.0
        }
        fn random(&mut self) -> f64 {
            0.0
        }
        fn image_get(&mut self, url: &str) -> Result<Vec<u8>, CellError> {
            if url == "https://example.com/logo.png" { self.bytes.clone() } else { Err(CellError::Connect) }
        }
        fn set_image(&mut self, picture: ImagePicture) {
            *self.set.borrow_mut() = Some(picture);
        }
    }

    fn png() -> Vec<u8> {
        b"\x89PNG\r\n\x1a\nrest".to_vec()
    }

    fn run(args: Vec<Arg>, bytes: Result<Vec<u8>, CellError>) -> (R<Value>, Option<ImagePicture>) {
        let mut ctx = Fetch { bytes, set: std::cell::RefCell::new(None) };
        let r = image(&args, &mut ctx);
        let set = ctx.set.into_inner();
        (r, set)
    }

    #[test]
    fn sniffs_supported_formats_only() {
        assert_eq!(sniff(&png()), Some("image/png"));
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(sniff(b"GIF89a...."), Some("image/gif"));
        assert_eq!(sniff(b"BM...."), Some("image/bmp"));
        assert_eq!(sniff(b"<svg>"), None);
        assert_eq!(sniff(b""), None);
    }

    #[test]
    fn returns_alt_text_and_hands_the_picture_to_the_host() {
        let (r, set) = run(vec![t("https://example.com/logo.png"), t("Logo")], Ok(png()));
        assert_eq!(r, Ok(Value::from("Logo")));
        assert_eq!(set, Some(ImagePicture { data: png(), mime: "image/png", alt: "Logo".into(), sizing: PictureSizing::Fit }));
    }

    #[test]
    fn rejects_bad_arguments_without_fetching() {
        for args in [
            vec![t("http://example.com/logo.png")],
            vec![t("https:///x")],
            vec![t("https://example.com/logo.png"), t("a"), t("4")],
            vec![t("https://example.com/logo.png"), t("a"), t("0"), t("10")],
            vec![t("https://example.com/logo.png"), t("a"), t("3"), t("10")],
            vec![t("https://example.com/logo.png"), t("a"), t("3"), t("0"), t("10")],
            vec![t("https://example.com/logo.png"), t("a"), t("3"), t("10"), t("20000")],
            vec![e(CellError::Div0)],
        ] {
            let (r, set) = run(args, Ok(png()));
            assert!(r.is_err(), "{r:?}");
            assert!(set.is_none());
        }
    }

    #[test]
    fn custom_sizing_carries_height_and_width() {
        let args = vec![t("https://example.com/logo.png"), t(""), t("3"), t("40"), t("60")];
        let (r, set) = run(args, Ok(png()));
        assert!(r.is_ok());
        assert_eq!(set.map(|p| p.sizing), Some(PictureSizing::Custom { width: 60, height: 40 }));
    }

    #[test]
    fn fetch_failures_are_blocked_or_connect() {
        assert_eq!(run(vec![t("https://example.com/logo.png")], Err(CellError::Blocked)).0, Err(CellError::Blocked));
        assert_eq!(run(vec![t("https://example.com/logo.png")], Err(CellError::Connect)).0, Err(CellError::Connect));
        assert_eq!(run(vec![t("https://example.com/other.png")], Ok(png())).0, Err(CellError::Connect));
        assert_eq!(CellError::from(FetchError::Blocked), CellError::Blocked);
        assert_eq!(CellError::from(FetchError::Connect), CellError::Connect);
    }

    #[test]
    fn unsupported_or_oversized_picture_is_value_error() {
        assert_eq!(run(vec![t("https://example.com/logo.png")], Ok(b"<svg/>".to_vec())).0, Err(CellError::Value));
        let too_big = vec![0u8; MAX_IMAGE_BYTES + 1];
        let (r, _) = run(vec![t("https://example.com/logo.png")], Ok([b"\x89PNG\r\n\x1a\n".to_vec(), too_big].concat()));
        assert_eq!(r, Err(CellError::Value));
    }
}
