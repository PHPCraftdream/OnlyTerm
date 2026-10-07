use crate::termwindow::TermWindowNotif;
use crate::TermWindow;
use onlyterm_config::keyassignment::{ClipboardCopyDestination, ClipboardPasteSource};
use onlyterm_mux::pane::Pane;
use onlyterm_mux::Mux;
use std::sync::Arc;
use window::{Clipboard, WindowOps};

impl TermWindow {
    pub fn copy_to_clipboard(&self, clipboard: ClipboardCopyDestination, text: String) {
        let window = self.window.as_ref().unwrap();
        match clipboard {
            ClipboardCopyDestination::Clipboard => window.set_clipboard(Clipboard::Clipboard, text),
            ClipboardCopyDestination::PrimarySelection => {
                window.set_clipboard(Clipboard::PrimarySelection, text)
            }
            ClipboardCopyDestination::ClipboardAndPrimarySelection => {
                window.set_clipboard(Clipboard::Clipboard, text.clone());
                window.set_clipboard(Clipboard::PrimarySelection, text);
            }
        }
    }

    pub(crate) fn copy_terminal_text_to_clipboard(
        &self,
        clipboard: ClipboardCopyDestination,
        text: String,
    ) {
        self.copy_to_clipboard(clipboard, trim_terminal_copy(text));
    }

    pub fn paste_from_clipboard(&mut self, pane: &Arc<dyn Pane>, clipboard: ClipboardPasteSource) {
        let pane_id = pane.pane_id();
        log::trace!(
            "paste_from_clipboard in pane {} {:?}",
            pane.pane_id(),
            clipboard
        );
        let window = self.window.as_ref().unwrap().clone();
        let clipboard = match clipboard {
            ClipboardPasteSource::Clipboard => Clipboard::Clipboard,
            ClipboardPasteSource::PrimarySelection => Clipboard::PrimarySelection,
        };
        let future = window.get_clipboard(clipboard);
        onlyterm_promise::spawn::spawn(async move {
            if let Ok(clip) = future.await {
                window.notify(TermWindowNotif::Apply(Box::new(move |myself| {
                    if let Some(pane) = myself
                        .pane_state(pane_id)
                        .overlay
                        .as_ref()
                        .map(|overlay| overlay.pane.clone())
                        .or_else(|| {
                            let mux = Mux::get();
                            mux.get_pane(pane_id)
                        })
                    {
                        pane.send_paste(&clip).ok();
                    }
                })));
            }
        })
        .detach();
        self.maybe_scroll_to_bottom_for_input(pane);
    }
}

fn trim_terminal_copy(mut text: String) -> String {
    let start = text.len() - text.trim_start().len();
    let len = text[start..].trim_end().len();
    text.truncate(start + len);
    if start != 0 {
        drop(text.drain(..start));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::trim_terminal_copy;

    #[test]
    fn copied_text_trims_unicode_edges_without_changing_inner_whitespace() {
        assert_eq!(
            trim_terminal_copy("\u{a0}\t Привет🙂 \n\n  indented\ttext\r\n\u{2003}".into()),
            "Привет🙂 \n\n  indented\ttext"
        );
    }

    #[test]
    fn copying_only_whitespace_produces_empty_text() {
        assert_eq!(trim_terminal_copy("\t \r\n\u{2003}\u{a0}".into()), "");
        assert_eq!(trim_terminal_copy(String::new()), "");
    }

    #[test]
    fn already_trimmed_text_preserves_internal_spacing() {
        assert_eq!(trim_terminal_copy("a  b\n\tc".into()), "a  b\n\tc");
    }
}
