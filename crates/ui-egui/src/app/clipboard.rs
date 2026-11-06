use gridcraft_engine::model::ObjKind;

use super::{Json, SheetApp, json};
use crate::published_picture::{PICTURE_COPIED_TEXT, color_image_hash, rgba_hash};

impl SheetApp {
    /// Copy or cut the selected picture or, with none selected, the selected cells, to the system
    /// clipboard. Engine-only commands keep their existing internal clipboard behavior and do not
    /// write to the host clipboard.
    pub fn copy_to_clipboard(&mut self, ctx: &egui::Context, command: &str) {
        if let Some(id) = self.selected_picture() {
            return self.copy_picture(ctx, id, command);
        }
        // Only ask the engine for HTML when the host can publish it.
        let result = match self.run_command(command, json!({"html": self.services.copy_html.is_some()})) {
            Ok(result) => result,
            Err(e) => return self.alert(e),
        };
        self.forget_published_picture();
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
    ///
    /// Text also ends a floating picture's copy, unless it is the text the system clipboard held
    /// when that copy was made.
    pub fn paste_external(&mut self, text: String) {
        if self.session.object_clipboard.is_some() {
            if Self::same_clipboard_text(Some(&text), self.clipboard_text_at_copy.as_deref()) {
                return self.paste_object();
            }
            self.forget_picture_copy();
        }
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
    /// text there but a picture, the picture is pasted, as [`Self::picture_paste_answered`] decides.
    /// With neither, no host clipboard (web), or the text that was there when GridCraft last copied
    /// without publishing, GridCraft's own clipboard is pasted.
    pub fn paste_from_clipboard(&mut self) {
        match self.system_clipboard_text() {
            Some(text) if self.session.clipboard.is_none() || self.clipboard_text_at_copy.as_ref() != Some(&text) => self.paste_external(text),
            Some(_) => self.run_or_alert("edit.paste", json!({})),
            None => match self.services.paste_image.as_mut().and_then(|read| read()) {
                Some(image) => {
                    if !self.picture_paste_answered(|| rgba_hash(&image)) {
                        self.paste_picture(image);
                    }
                }
                None if self.session.object_clipboard.is_some() => self.paste_object(),
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
        if self.picture_paste_answered(|| color_image_hash(image)) {
            return;
        }
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

    /// The selected object when it is a picture on the active sheet.
    fn selected_picture(&self) -> Option<u32> {
        let o = self.selected_object.filter(|o| o.kind == ObjKind::Image)?;
        self.session.active()?.wb.active()?.has_object(o).then_some(o.id)
    }

    /// Copy or cut a floating picture: onto the object clipboard, which keeps it linked to its Python
    /// cell, and its pixels onto the system clipboard, for other applications and for Paste to find.
    fn copy_picture(&mut self, ctx: &egui::Context, id: u32, command: &str) {
        // Before a cut takes it away.
        let picture = self.session.active().and_then(|d| d.wb.active()).and_then(|sh| sh.images.iter().find(|i| i.id == id));
        let pixels = picture.and_then(|p| crate::floating_pictures::rgba(&p.data));
        let command = if command == "edit.cut" { "object.cut" } else { "object.copy" };
        if let Err(e) = self.run_command(command, json!({"kind": "image", "id": id})) {
            return self.alert(e);
        }
        self.forget_published_picture();
        self.clipboard_text_at_copy = None;
        self.cut_text = None;
        match pixels.filter(|pixels| self.publish_picture(pixels)) {
            Some(pixels) => {
                let hash = self.hash_as_read_back(&pixels);
                self.published_picture = Some(hash);
                if let Some(note) = &mut self.services.note_published_picture {
                    note(Some(hash));
                }
            }
            None => {
                ctx.copy_text(PICTURE_COPIED_TEXT.to_string());
                self.clipboard_text_at_copy = Some(PICTURE_COPIED_TEXT.to_string());
            }
        }
    }

    fn publish_picture(&mut self, pixels: &image::RgbaImage) -> bool {
        let Some(copy) = &mut self.services.copy_image else { return false };
        match copy(pixels) {
            Ok(()) => true,
            Err(error) => {
                log::warn!("Picture clipboard unavailable: {error}");
                false
            }
        }
    }

    /// The hash of the picture just published, as a later read will see it. A host may not give back
    /// exactly what it was given (colour matching, transparency written premultiplied), and the hash
    /// has to match what Paste finds. A picture of another size is not what was published.
    fn hash_as_read_back(&mut self, pixels: &image::RgbaImage) -> u64 {
        let back = self.services.paste_image.as_mut().and_then(|read| read());
        rgba_hash(back.as_ref().filter(|b| b.dimensions() == pixels.dimensions()).unwrap_or(pixels))
    }

    /// `object.copy` or `object.cut` from an agent leaves the system clipboard alone: note what it
    /// holds, so Paste can tell that the picture is the newer one while that is still there.
    pub(super) fn note_clipboard_at_object_copy(&mut self) {
        self.forget_published_picture();
        self.clipboard_text_at_copy = self.system_clipboard_text();
        self.cut_text = None;
        if self.clipboard_text_at_copy.is_none() {
            self.published_picture = self.services.paste_image.as_mut().and_then(|read| read()).as_ref().map(rgba_hash);
        }
    }

    /// Paste a picture from the object clipboard.
    fn paste_object(&mut self) {
        self.run_or_alert("object.paste", json!({}));
    }

    /// What the object clipboard says about a picture found on the system clipboard, whose hash
    /// `hash` gives. True when that settles it: the picture this process published is the object
    /// clipboard's to paste or refuse, and one another process published is refused, because that
    /// process's workbook is another workbook. Any other picture came from elsewhere, so the object
    /// clipboard is forgotten and it is pasted as a new static picture.
    fn picture_paste_answered(&mut self, hash: impl FnOnce() -> u64) -> bool {
        let published = self.published_picture.filter(|_| self.session.object_clipboard.is_some());
        let noted = self.services.read_published_picture.as_mut().and_then(|read| read());
        if published.is_some() || noted.is_some() || self.copied_image.is_some() {
            let hash = hash();
            if let Some((_, doc)) = self.copied_image.filter(|(copied, _)| *copied == hash) {
                if self.session.active().map(|d| d.uid) == Some(doc) {
                    return false;
                }
                self.alert(gridcraft_engine::OTHER_WORKBOOK.to_string());
                return true;
            }
            if published == Some(hash) {
                self.paste_object();
                return true;
            }
            if noted == Some(hash) {
                self.alert(gridcraft_engine::OTHER_WORKBOOK.to_string());
                return true;
            }
        }
        self.forget_picture_copy();
        false
    }

    /// The system clipboard has moved on from the floating picture copied last.
    pub(crate) fn forget_picture_copy(&mut self) {
        if self.session.object_clipboard.is_some() {
            self.clipboard_text_at_copy = None;
        }
        self.session.forget_object_clipboard();
        self.forget_published_picture();
    }

    /// Copy Image put these pixels on the clipboard, from this workbook. Paste keeps them in it, and the
    /// note keeps them out of another process's workbooks.
    pub(crate) fn note_copied_image(&mut self, image: &image::RgbaImage) {
        let hash = rgba_hash(image);
        self.copied_image = self.session.active().map(|d| (hash, d.uid));
        if let Some(note) = &mut self.services.note_published_picture {
            note(Some(hash));
        }
    }

    /// The hash of the published picture no longer stands. Another process's note stays.
    pub(super) fn forget_published_picture(&mut self) {
        let Some(hash) = self.published_picture.take().or_else(|| self.copied_image.take().map(|(hash, _)| hash)) else { return };
        self.copied_image = None;
        if self.services.read_published_picture.as_mut().and_then(|read| read()) == Some(hash)
            && let Some(note) = &mut self.services.note_published_picture
        {
            note(None);
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
