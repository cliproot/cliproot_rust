pub mod html;
mod error;

pub use error::ClipboardError;

/// Result of a clipboard write operation.
pub enum WriteResult {
    /// text/plain + text/html with embedded data-crp-bundle (Approach A)
    HtmlOnly,
}

pub struct ClipboardWriter {
    inner: arboard::Clipboard,
}

impl ClipboardWriter {
    pub fn new() -> Result<Self, ClipboardError> {
        Ok(Self {
            inner: arboard::Clipboard::new()?,
        })
    }

    /// Write plain text only — no provenance metadata.
    pub fn write_plain(&mut self, text: &str) -> Result<(), ClipboardError> {
        self.inner.set_text(text.to_string())?;
        Ok(())
    }

    /// Approach A: text/plain + text/html with hidden div carrying data-crp-bundle.
    ///
    /// Mirrors `writeProvenanceToClipboard` from `clipboard-writer.ts`.
    /// Works on all platforms; no process-lifetime concern (arboard manages the
    /// background thread on Linux/X11 automatically).
    pub fn write_with_html(
        &mut self,
        plain: &str,
        bundle_json: &str,
    ) -> Result<WriteResult, ClipboardError> {
        let html_payload = html::build_provenance_html(plain, bundle_json);
        self.inner
            .set()
            .html(html_payload, Some(plain.to_string()))?;
        Ok(WriteResult::HtmlOnly)
    }
}
