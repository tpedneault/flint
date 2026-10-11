//! The clipboard, through `arboard`.
//!
//! On X11 and Wayland the clipboard's contents belong to the program that
//! set them and vanish when it lets go, so the daemon keeps one
//! [`Clipboard`] for its whole life: the text `y` copied stays pasteable
//! after Flint hides. It is opened on first use, not at start, so a session
//! without a clipboard costs nothing until someone copies.

/// The system clipboard.
#[derive(Default)]
pub struct Clipboard {
    inner: Option<arboard::Clipboard>,
}

impl Clipboard {
    /// A clipboard, opened when first used.
    pub fn new() -> Self {
        Self::default()
    }

    /// Puts `text` on the clipboard.
    pub fn set(&mut self, text: &str) -> Result<(), String> {
        if self.inner.is_none() {
            self.inner = Some(arboard::Clipboard::new().map_err(|e| format!("no clipboard: {e}"))?);
        }
        let clipboard = self.inner.as_mut().ok_or("no clipboard")?;
        clipboard
            .set_text(text.to_owned())
            .map_err(|e| format!("couldn't copy: {e}"))
    }
}
