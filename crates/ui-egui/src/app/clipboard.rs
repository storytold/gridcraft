use super::{Json, SheetApp, json};

impl SheetApp {
    /// Copy or cut the selected cells to the system clipboard. Engine-only commands keep
    /// their existing internal clipboard behavior and do not write to the host clipboard.
    pub fn copy_to_clipboard(&mut self, ctx: &egui::Context, command: &str) {
        self.copy_to_clipboard_ok(ctx, command);
    }

    /// [`Self::copy_to_clipboard`], telling whether the command ran (an error is alerted and is `false`).
    pub(super) fn copy_to_clipboard_ok(&mut self, ctx: &egui::Context, command: &str) -> bool {
        // Only ask the engine for HTML when the host can publish it.
        let result = match self.run_command(command, json!({"html": self.services.copy_html.is_some()})) {
            Ok(result) => result,
            Err(e) => {
                self.alert(e);
                return false;
            }
        };
        let Some(text) = result.get("text").and_then(Json::as_str) else { return true };
        // The system clipboard gets this copy, so its text decides what Paste pastes.
        self.clipboard_text_at_copy = None;
        self.cut_text = (command == "edit.cut").then(|| text.replace("\r\n", "\n"));
        if let Some(html) = result.get("html").and_then(Json::as_str)
            && let Some(copy_html) = &mut self.services.copy_html
        {
            match copy_html(html, text) {
                Ok(()) => return true,
                Err(error) => log::warn!("HTML clipboard unavailable; copying plain text: {error}"),
            }
        }
        ctx.copy_text(text.to_string());
        true
    }

    /// Paste text that came from the system clipboard (Cmd/Ctrl+V, or the ribbon and menu Paste),
    /// with the clipboard's HTML flavour when the host can read it.
    ///
    /// Excel pastes a cut once: after GridCraft's cut has been pasted, the system clipboard text
    /// that came with it is not pasted again over the moved block; GridCraft's own (empty)
    /// clipboard is pasted instead, which reports "Nothing to paste.". Other text means the
    /// clipboard has moved on, so the cut's text is forgotten and pastes normally later.
    pub fn paste_external(&mut self, text: String) {
        self.paste_external_ok(text);
    }

    /// [`Self::paste_external`], telling whether the paste ran.
    fn paste_external_ok(&mut self, text: String) -> bool {
        if Self::same_clipboard_text(Some(&text), self.cut_text.as_deref()) {
            if self.session.clipboard.is_none() {
                return self.run_or_alert_ok("edit.paste", json!({}));
            }
        } else {
            self.cut_text = None;
        }
        let html = self.services.paste_html.as_mut().and_then(|read| read());
        let params = match html {
            Some(html) => json!({"text": text, "html": html}),
            None => json!({"text": text}),
        };
        self.run_or_alert_ok("edit.paste", params)
    }

    /// Ribbon and menu Paste: paste the text on the system clipboard, as Cmd/Ctrl+V does. With no
    /// text there, no host clipboard (web), or the text that was there when GridCraft last copied
    /// without publishing, GridCraft's own clipboard is pasted.
    pub fn paste_from_clipboard(&mut self) {
        self.paste_from_clipboard_ok();
    }

    /// [`Self::paste_from_clipboard`], telling whether the paste ran.
    pub(super) fn paste_from_clipboard_ok(&mut self) -> bool {
        match self.system_clipboard_text() {
            Some(text) if self.session.clipboard.is_none() || self.clipboard_text_at_copy.as_ref() != Some(&text) => self.paste_external_ok(text),
            _ => self.run_or_alert_ok("edit.paste", json!({})),
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
