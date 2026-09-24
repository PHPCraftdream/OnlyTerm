use crate::config::NewlineCanon;
use crate::terminalstate::*;

impl TerminalState {
    pub(crate) fn set_clipboard_contents(
        &self,
        selection: ClipboardSelection,
        text: Option<String>,
    ) -> anyhow::Result<()> {
        if let Some(clip) = self.clipboard.as_ref() {
            clip.set_contents(selection, text)?;
        }
        Ok(())
    }

    /// Send text to the terminal that is the result of pasting.
    /// If bracketed paste mode is enabled, the paste is enclosed
    /// De-fang the text by removing any embedded bracketed paste
    /// sequence that may be present.  Loops until no more occurrences
    /// remain, because a single pass of str::replace is not idempotent:
    /// nested sequences like `\x1b\x1b[200~[200~` can leave a valid
    /// marker behind after the first sweep.
    pub(crate) fn defang_paste(text: &str) -> String {
        let mut result = text.to_string();
        loop {
            let prev = result.clone();
            result = result.replace("\x1b[200~", "").replace("\x1b[201~", "");
            if result == prev {
                break;
            }
        }
        result
    }

    /// in the bracketing, otherwise it is fed to the writer as-is.
    pub fn send_paste(&mut self, text: &str) -> Result<(), Error> {
        let mut buf = String::new();
        if self.bracketed_paste {
            buf.push_str("\x1b[200~");
        }

        let canon = if self.bracketed_paste {
            NewlineCanon::None
        } else {
            self.config.canonicalize_pasted_newlines()
        };

        let canon = canon.canonicalize(text);
        let de_fanged = Self::defang_paste(&canon);
        buf.push_str(&de_fanged);

        if self.bracketed_paste {
            buf.push_str("\x1b[201~");
        }

        self.writer.write_all(buf.as_bytes())?;
        self.writer.flush()?;
        Ok(())
    }
}
