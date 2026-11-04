use super::{Json, SheetApp, json};

impl SheetApp {
    /// Copy or cut the selected cells to the system clipboard. Engine-only commands keep
    /// their existing internal clipboard behavior and do not write to the host clipboard.
    pub fn copy_to_clipboard(&mut self, ctx: &egui::Context, command: &str) {
        // Only ask the engine for HTML when the host can publish it.
        let result = match self.run_command(command, json!({"html": self.services.copy_html.is_some()})) {
            Ok(result) => result,
            Err(e) => return self.alert(e),
        };
        let Some(text) = result.get("text").and_then(Json::as_str) else { return };
        // The system clipboard gets this copy, so its text decides what Paste pastes.
        self.clipboard_text_at_copy = None;
        self.cut_text = (command == "edit.cut").then(|| text.replace("\r\n", "\n"));
        if let Some(html) = result.get("html").and_then(Json::as_str)
            && let Some(copy_html) = &mut self.services.copy_html
        {
            match copy_html(html, text) {
                Ok(()) => return,
                Err(error) => log::warn!("HTML clipboard unavailable; copying plain text: {error}"),
            }
        }
        ctx.copy_text(text.to_string());
    }

    /// Paste text that came from the system clipboard (Cmd/Ctrl+V, or the ribbon and menu Paste),
    /// with the clipboard's HTML flavour when the host can read it.
    ///
    /// Excel pastes a cut once: after GridCraft's cut has been pasted, the system clipboard text
    /// that came with it is not pasted again over the moved block; GridCraft's own (empty)
    /// clipboard is pasted instead, which reports "Nothing to paste.". Other text means the
    /// clipboard has moved on, so the cut's text is forgotten and pastes normally later.
    pub fn paste_external(&mut self, text: String) {
        if Self::same_clipboard_text(Some(&text), self.cut_text.as_deref()) {
            if self.session.clipboard.is_none() {
                return self.run_or_alert("edit.paste", json!({}));
            }
        } else {
            self.cut_text = None;
        }
        let html = self.services.paste_html.as_mut().and_then(|read| read());
        let params = match html {
            Some(html) => json!({"text": text, "html": html}),
            None => json!({"text": text}),
        };
        self.run_or_alert("edit.paste", params);
    }

    /// Ribbon and menu Paste: paste the text on the system clipboard, as Cmd/Ctrl+V does. With no
    /// text there but a picture, the picture is pasted. With neither, no host clipboard (web), or
    /// the text that was there when GridCraft last copied without publishing, GridCraft's own
    /// clipboard is pasted.
    pub fn paste_from_clipboard(&mut self) {
        match self.system_clipboard_text() {
            Some(text) if self.session.clipboard.is_none() || self.clipboard_text_at_copy.as_ref() != Some(&text) => self.paste_external(text),
            Some(_) => self.run_or_alert("edit.paste", json!({})),
            None => match self.services.paste_image.as_mut().and_then(|read| read()) {
                Some(image) => self.paste_picture(image),
                None => self.run_or_alert("edit.paste", json!({})),
            },
        }
    }

    /// Paste a picture from the system clipboard (a screenshot, an image copied in a browser) as
    /// a floating picture whose top-left corner is the active cell, as Excel does.
    pub fn paste_picture(&mut self, image: image::RgbaImage) {
        match encode_png(&image) {
            Ok(png) => self.run_or_alert("insert.picture", json!({"base64": gridcraft_engine::io::base64_encode(&png), "mime": "image/png"})),
            Err(e) => self.alert(e),
        }
    }

    /// Cmd/Ctrl+V with a picture and no text on the clipboard (egui's `Event::PasteImage`).
    pub fn paste_color_image(&mut self, image: &egui::ColorImage) {
        let [w, h] = image.size;
        // A side too large for u32 is too large to paste; check before copying the pixels.
        let (w, h) = (u32::try_from(w).unwrap_or(u32::MAX), u32::try_from(h).unwrap_or(u32::MAX));
        if let Err(e) = check_picture_size(w, h) {
            return self.alert(e);
        }
        let rgba = image.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
        match image::RgbaImage::from_raw(w, h, rgba) {
            Some(image) => self.paste_picture(image),
            None => self.alert("The picture on the clipboard can't be read.".into()),
        }
    }

    /// The system clipboard's plain text, with the line endings of egui's `Event::Paste` text.
    /// `None` when it holds no text or the host can't read it.
    pub(super) fn system_clipboard_text(&mut self) -> Option<String> {
        self.services.paste_text.as_mut().and_then(|read| read()).filter(|t| !t.is_empty()).map(|t| t.replace("\r\n", "\n"))
    }

    /// Whether two clipboard texts are the same copy: trailing line breaks don't count, as in the
    /// engine's `edit.paste`, and CRLF matches LF (egui's `Event::Paste` text may keep the host's).
    fn same_clipboard_text(a: Option<&str>, b: Option<&str>) -> bool {
        fn chars(t: &str) -> impl Iterator<Item = char> + '_ {
            t.trim_end_matches(['\r', '\n']).chars().filter(|c| *c != '\r')
        }
        match (a, b) {
            (Some(a), Some(b)) => chars(a).eq(chars(b)),
            _ => false,
        }
    }
}

/// Largest clipboard picture pasted, per side and in total (an 8K screenshot fits).
const MAX_PICTURE_SIDE: u32 = 16_384;
const MAX_PICTURE_PIXELS: u64 = 64 * 1024 * 1024;

/// Refuse empty clipboard pictures and sizes that would exhaust memory.
fn check_picture_size(w: u32, h: u32) -> Result<(), String> {
    if w == 0 || h == 0 {
        return Err("The picture on the clipboard is empty.".into());
    }
    if w > MAX_PICTURE_SIDE || h > MAX_PICTURE_SIDE || u64::from(w) * u64::from(h) > MAX_PICTURE_PIXELS {
        return Err("The picture on the clipboard is too large to paste.".into());
    }
    Ok(())
}

/// PNG bytes of a clipboard picture, refusing sizes that would exhaust memory.
fn encode_png(image: &image::RgbaImage) -> Result<Vec<u8>, String> {
    let (w, h) = image.dimensions();
    check_picture_size(w, h)?;
    let mut png = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| format!("The picture on the clipboard can't be pasted: {e}"))?;
    Ok(png)
}
