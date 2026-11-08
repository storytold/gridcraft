//! Brush textures: importing, listing, removing, and what hostile input gets refused.

use std::io::Cursor;

use serde_json::json;

use super::s;

fn png(w: u32, h: u32) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(w, h, image::Rgba([30, 120, 200, 255]));
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

fn b64(bytes: &[u8]) -> String {
    crate::io::base64_encode(bytes)
}

#[test]
fn an_imported_texture_is_listed_and_reused() {
    let mut s = s();
    let bytes = png(16, 8);
    let first = s.execute("draw.textureImport", json!({"name": "stripes.png", "base64": b64(&bytes)})).unwrap();
    assert_eq!(first["added"], true);
    assert_eq!(first["width"], 16);
    assert_eq!(first["height"], 8);
    let id = first["id"].as_str().unwrap().to_string();
    assert_eq!(id.len(), 32);
    let again = s.execute("draw.textureImport", json!({"name": "same.png", "base64": b64(&bytes)})).unwrap();
    assert_eq!(again["added"], false, "the same bytes reuse the entry");
    assert_eq!(again["id"].as_str().unwrap(), id);
    let list = s.execute("draw.textureList", json!({})).unwrap();
    assert_eq!(list["textures"].as_array().unwrap().len(), 1);
    assert_eq!(list["textures"][0]["name"], "stripes.png");
    assert_eq!(list["textures"][0]["mime"], "image/png");
    assert_eq!(list["textures"][0]["bytes"].as_u64().unwrap() as usize, bytes.len());
}

#[test]
fn removing_a_texture_forgets_it_and_clears_the_pen() {
    let mut s = s();
    let id = s.execute("draw.textureImport", json!({"name": "a.png", "base64": b64(&png(4, 4))})).unwrap()["id"].as_str().unwrap().to_string();
    let pen = s.execute("draw.pen", json!({"texture": id, "textureScale": 3})).unwrap();
    assert_eq!(pen["texture"], id.as_str());
    assert_eq!(pen["textureScale"], 3.0);
    let removed = s.execute("draw.textureRemove", json!({"id": id})).unwrap();
    assert_eq!(removed["removed"], true);
    assert_eq!(s.execute("draw.textureRemove", json!({"id": id})).unwrap()["removed"], false);
    assert_eq!(s.execute("draw.textureList", json!({})).unwrap()["textures"].as_array().unwrap().len(), 0);
    assert_eq!(s.execute("draw.textureList", json!({})).unwrap()["selected"], "");
    // The pen no longer names a texture that is gone.
    assert!(s.execute("draw.pen", json!({"texture": id})).is_err());
}

#[test]
fn importing_is_one_undo_step() {
    let mut s = s();
    let bytes = png(8, 8);
    s.execute("draw.textureImport", json!({"name": "a.png", "base64": b64(&bytes)})).unwrap();
    assert_eq!(s.doc().unwrap().wb.ink_textures.len(), 1);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.doc().unwrap().wb.ink_textures.len(), 0);
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(s.doc().unwrap().wb.ink_textures.len(), 1);
}

#[test]
fn hostile_imports_are_refused() {
    let mut s = s();
    for (params, why) in [
        (json!({"name": "x.png", "base64": "!!!!"}), "invalid base64"),
        (json!({"name": "x.png", "base64": b64(b"not an image at all")}), "not an image"),
        (json!({"name": "x.gif", "base64": b64(b"GIF89a\x01\x00\x01\x00\x00\x00\x00")}), "not a PNG or JPEG"),
        (json!({"name": "x.png", "base64": b64(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3])}), "truncated"),
        (json!({"name": "x.png", "base64": b64(&vec![0u8; 16 * 1024 * 1024 + 1])}), "too large"),
        (json!({}), "no source"),
    ] {
        assert!(s.execute("draw.textureImport", params).is_err(), "{why} must be refused");
    }
    assert_eq!(s.doc().unwrap().wb.ink_textures.len(), 0);
}

#[test]
fn the_pen_scale_is_kept_within_its_limits() {
    let mut s = s();
    let id = s.execute("draw.textureImport", json!({"name": "a.png", "base64": b64(&png(4, 4))})).unwrap()["id"].as_str().unwrap().to_string();
    assert_eq!(s.execute("draw.pen", json!({"texture": id, "textureScale": 0.001})).unwrap()["textureScale"], 0.5);
    assert_eq!(s.execute("draw.pen", json!({"textureScale": 10_000})).unwrap()["textureScale"], 512.0);
    assert_eq!(s.execute("draw.pen", json!({"textureScale": "junk"})).unwrap()["textureScale"], 512.0, "a bad value keeps the pen's");
    assert_eq!(s.execute("draw.pen", json!({"texture": ""})).unwrap()["texture"], "", "an empty id clears the texture");
}

#[test]
fn textures_travel_in_the_workbook_json() {
    let mut s = s();
    let bytes = png(6, 4);
    s.execute("draw.textureImport", json!({"name": "json.png", "base64": b64(&bytes)})).unwrap();
    let saved = s.execute("file.saveBytes", json!({"format": "json"})).unwrap();
    let mut back = s();
    back.execute("file.open", json!({"name": "book.json", "base64": saved["base64"]})).unwrap();
    let list = back.execute("draw.textureList", json!({})).unwrap();
    assert_eq!(list["textures"].as_array().unwrap().len(), 1);
    assert_eq!(list["textures"][0]["name"], "json.png");
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn a_texture_imports_from_a_path() {
    let dir = std::env::temp_dir().join(format!("gridcraft-texture-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("stripes.png");
    std::fs::write(&path, png(9, 5)).unwrap();
    let mut s = s();
    let r = s.execute("draw.textureImport", json!({"path": path.to_string_lossy()})).unwrap();
    assert_eq!(r["name"], "stripes.png", "the file name names the texture");
    assert_eq!(r["width"], 9);
    std::fs::remove_dir_all(&dir).ok();
}
