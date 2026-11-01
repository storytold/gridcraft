//! The From Web background fetch: page downloads run on a worker thread and their results
//! re-enter through the normal engine commands as `prefetchedByUrl` (tables keyed by the URL
//! they came from), so a slow page never blocks the UI (Excel runs its queries in the
//! background too). The web build has no threads: its fetches stay on the synchronous path.

use serde_json::{Value as Json, json};

use crate::SheetApp;

/// One in-flight fetch: the URLs to download and the command its results feed.
pub struct PendingFetch {
    urls: Vec<String>,
    /// The window the fetch was started in: only that window's pass polls or discards the
    /// finished fetch (another window drawing first must not consume a result the owning
    /// window's dialog still expects).
    pub(crate) window: Option<u64>,
    /// The document the fetch was started for: if the user switches workbooks while the page
    /// downloads, the result must not run its command against the newly active one.
    document_uid: Option<u64>,
    /// The command the results feed (`data.webPreview`, `data.webRefresh`).
    pub(crate) command: (String, Json),
    handle: Option<std::thread::JoinHandle<Json>>,
}

/// The fetched tables of one page, as the `prefetchedByUrl` value holds them.
fn table_json(t: &gridcraft_engine::webquery::WebTable) -> Json {
    json!({"name": t.name, "rows": t.rows})
}

impl SheetApp {
    /// Starts fetching `urls` in the background (a no-op retry if the same fetch for the same
    /// command is already running); [`poll_fetch`](Self::poll_fetch) runs `command` with the
    /// results once they are in. An empty URL list starts nothing: there is nothing to fetch.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn start_fetch(&mut self, urls: &[&str], command: (&str, Json)) {
        if urls.is_empty() {
            return;
        }
        let urls: Vec<String> = urls.iter().map(|s| s.to_string()).collect();
        if self.fetch.as_ref().is_some_and(|f| f.urls == urls && f.command.0 == command.0 && f.command.1 == command.1) {
            return;
        }
        // A different fetch while one runs would drop the running one's handle (its result lost,
        // its dialog left claiming "No tables were found"), so refuse it instead. One deliberate
        // exception: Load before Go replaces this dialog's own preview for the same address —
        // the preview's result is worthless once its import is on the way.
        let replaces_own_preview = self.fetch.as_ref().is_some_and(|f| f.command.0 == "data.webPreview" && command.0 == "data.fromWeb" && f.urls == urls);
        if self.fetching() && !replaces_own_preview {
            self.message = Some(("From Web".into(), "A page is still loading. Try again when it finishes.".into()));
            return;
        }
        let source = match self.session.web_source.clone() {
            Some(s) => s,
            None => {
                self.message = Some(("From Web".into(), "Web page import is not available in this build.".into()));
                return;
            }
        };
        let fetch_urls = urls.clone();
        let handle = std::thread::spawn(move || {
            // Each page lands under its own URL; a failed page records its error instead.
            let mut map = serde_json::Map::new();
            for url in &fetch_urls {
                let req = gridcraft_engine::webquery::WebRequest {
                    url: url.clone(),
                    max_rows: gridcraft_engine::webquery::DEFAULT_MAX_ROWS,
                    max_cols: gridcraft_engine::webquery::MAX_COLS,
                    max_bytes: gridcraft_engine::webquery::DEFAULT_MAX_BYTES,
                };
                let entry = match source(&req) {
                    Ok(tables) => json!({"tables": tables.iter().map(table_json).collect::<Vec<_>>()}),
                    Err(e) => json!({"error": e}),
                };
                map.insert(url.clone(), entry);
            }
            Json::Object(map)
        });
        self.fetch = Some(PendingFetch { urls, window: self.ui_window(), document_uid: self.session.active().map(|d| d.uid), command: (command.0.to_string(), command.1), handle: Some(handle) });
    }

    /// True while a fetch is still downloading, for a spinner and repaints.
    pub fn fetching(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.fetch.as_ref().is_some_and(|f| f.handle.as_ref().is_some_and(|h| !h.is_finished()))
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    /// Discards a fetch without running its command, whether or not the worker has finished:
    /// the handle is dropped, never joined. Joining an unfinished worker on the UI thread
    /// would block the whole interface until the page download ends (a Refresh All runs its
    /// URLs one after another); a dropped handle detaches the thread instead (std reclaims
    /// it on exit), and the worker's result is already going nowhere — the channel's
    /// receiver went with the fetch.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn discard_fetch(&mut self) {
        drop(self.fetch.take());
    }

    /// Applies a finished fetch: the downloaded pages go to `command` as `prefetchedByUrl` and
    /// both the raw map and the command's result come back to the caller.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn poll_fetch(&mut self) -> Option<(Json, Result<Json, String>)> {
        if !self.fetch.as_ref().is_some_and(|f| f.handle.as_ref().is_none_or(|h| h.is_finished())) {
            return None;
        }
        let fetch = self.fetch.take()?;
        let pages = fetch.handle.and_then(|h| h.join().ok());
        let (id, mut params) = fetch.command;
        match pages {
            Some(pages) => {
                // The workbook the fetch was started for: if another is active now, the import
                // would land in it instead. Report the mismatch without running the command.
                if self.session.active().map(|d| d.uid) != fetch.document_uid {
                    return Some((pages, Err("The workbook changed while the page was loading.".into())));
                }
                params.as_object_mut().map(|o| o.insert("prefetchedByUrl".into(), pages.clone()));
                Some((pages, self.run(&id, params)))
            }
            None => {
                self.message = Some(("From Web".into(), "The page fetch was cancelled.".into()));
                Some((Json::Null, Ok(Json::Null)))
            }
        }
    }
}
