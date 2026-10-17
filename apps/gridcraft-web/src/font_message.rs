//! The answer to a `gridcraft:font` message, as plain JSON (no browser types, so it is tested
//! natively); `host` posts it to the parent page.

use gridcraft_ui_egui::theme::FontAdded;
use serde_json::{Value as Json, json};

/// The `type` of the answer.
pub const FONT_RESULT: &str = "gridcraft:fontResult";

/// The answer for the font `name`: `{type, name, ok: true, added, covers}` when it was added or
/// a font of that name already was (`added` false), `{type, name, ok: false, error}` when it was
/// refused.
pub fn font_result(name: &str, result: &Result<FontAdded, String>) -> Json {
    let mut answer = match result {
        Ok(added) => added.to_json(name),
        Err(error) => json!({"name": name.trim(), "error": error}),
    };
    if let Some(fields) = answer.as_object_mut() {
        fields.insert("type".into(), FONT_RESULT.into());
        fields.insert("ok".into(), result.is_ok().into());
    }
    answer
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_ui_egui::theme::Coverage;

    #[test]
    fn the_page_learns_whether_its_font_was_added_ignored_or_refused() {
        let korean = Coverage { hangul: true, han: false, kana: false };
        assert_eq!(
            font_result("Noto Sans KR", &Ok(FontAdded { added: true, covers: korean })),
            json!({"type": "gridcraft:fontResult", "name": "Noto Sans KR", "ok": true, "added": true, "covers": {"hangul": true, "han": false, "kana": false}})
        );
        assert_eq!(font_result("Noto Sans KR", &Ok(FontAdded { added: false, covers: korean }))["added"], false, "a name added before");
        assert_eq!(
            font_result("Broken", &Err("isn't a font".into())),
            json!({"type": "gridcraft:fontResult", "name": "Broken", "ok": false, "error": "isn't a font"})
        );
    }
}
