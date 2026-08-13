use codec::*;
use mux::pane::{CachePolicy, Pane};
use mux::renderable::{RenderableDimensions, StableCursorPosition};
use std::sync::{Arc, Mutex};
use termwiz::input::KeyboardEncoding;
use termwiz::surface::SequenceNo;
use url::Url;
use wezterm_term::terminal::Alert;
use wezterm_term::StableRowIndex;

use super::PduSender;

#[derive(Default, Debug)]
pub(crate) struct PerPane {
    cursor_position: StableCursorPosition,
    title: String,
    working_dir: Option<Url>,
    dimensions: RenderableDimensions,
    mouse_grabbed: bool,
    /// Last keyboard encoding we told this client about. `None` until the
    /// first response is computed, so the very first push always carries the
    /// current value (which matters when the application negotiated its
    /// protocol before this client attached).
    keyboard_encoding: Option<KeyboardEncoding>,
    sent_initial_palette: bool,
    seqno: SequenceNo,
    config_generation: usize,
    pub(crate) notifications: Vec<Alert>,
}

impl PerPane {
    pub(super) fn compute_changes(
        &mut self,
        pane: &Arc<dyn Pane>,
        force_with_input_serial: Option<InputSerial>,
    ) -> Option<GetPaneRenderChangesResponse> {
        let mut changed = false;
        let mouse_grabbed = pane.is_mouse_grabbed();
        if mouse_grabbed != self.mouse_grabbed {
            changed = true;
        }

        // An application negotiating (or dropping) win32-input-mode / the
        // kitty keyboard protocol may not touch the screen at all, so this
        // has to participate in the change detection in its own right --
        // otherwise the client would keep encoding synthetic key events for
        // the wrong protocol until some unrelated output happened to force a
        // push.
        let keyboard_encoding = pane.get_keyboard_encoding();
        if Some(keyboard_encoding) != self.keyboard_encoding {
            changed = true;
        }

        let dims = pane.get_dimensions();
        if dims != self.dimensions {
            changed = true;
        }

        let cursor_position = pane.get_cursor_position();
        if cursor_position != self.cursor_position {
            changed = true;
        }

        let title = pane.get_title();
        if title != self.title {
            changed = true;
        }

        let working_dir = pane.get_current_working_dir(CachePolicy::AllowStale);
        if working_dir != self.working_dir {
            changed = true;
        }

        let old_seqno = self.seqno;
        self.seqno = pane.get_current_seqno();
        let mut all_dirty_lines = pane.get_changed_since(
            0..dims.physical_top + dims.viewport_rows as StableRowIndex,
            old_seqno,
        );
        if !all_dirty_lines.is_empty() {
            changed = true;
        }

        if !changed && !force_with_input_serial.is_some() {
            return None;
        }

        // Figure out what we're going to send as dirty lines vs bonus lines
        let viewport_range =
            dims.physical_top..dims.physical_top + dims.viewport_rows as StableRowIndex;

        let (first_line, lines) = pane.get_lines(viewport_range);
        let mut bonus_lines = lines
            .into_iter()
            .enumerate()
            .filter_map(|(idx, mut line)| {
                let stable_row = first_line + idx as StableRowIndex;
                if all_dirty_lines.contains(stable_row) {
                    all_dirty_lines.remove(stable_row);
                    line.compress_for_scrollback();
                    Some((stable_row, line))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        // Always send the cursor's row, as that tends to the busiest and we don't
        // have a sequencing concept for our idea of the remote state.
        let (cursor_line_idx, mut lines) = pane.get_lines(cursor_position.y..cursor_position.y + 1);
        let mut cursor_line = lines.remove(0);
        cursor_line.compress_for_scrollback();
        bonus_lines.push((cursor_line_idx, cursor_line));

        self.cursor_position = cursor_position;
        self.title = title.clone();
        self.working_dir = working_dir.clone();
        self.dimensions = dims;
        self.mouse_grabbed = mouse_grabbed;
        self.keyboard_encoding = Some(keyboard_encoding);

        let bonus_lines = bonus_lines.into();
        Some(GetPaneRenderChangesResponse {
            pane_id: pane.pane_id(),
            mouse_grabbed,
            dirty_lines: all_dirty_lines.iter().cloned().collect(),
            dimensions: dims,
            cursor_position,
            title,
            bonus_lines,
            working_dir: working_dir.map(Into::into),
            input_serial: force_with_input_serial,
            seqno: self.seqno,
            user_vars: pane.copy_user_vars(),
            keyboard_encoding,
        })
    }
}

pub(super) fn maybe_push_pane_changes(
    pane: &Arc<dyn Pane>,
    sender: PduSender,
    per_pane: Arc<Mutex<PerPane>>,
) -> anyhow::Result<()> {
    let mut per_pane = per_pane.lock().unwrap();
    if let Some(resp) = per_pane.compute_changes(pane, None) {
        sender.send(DecodedPdu {
            pdu: Pdu::GetPaneRenderChangesResponse(resp),
            serial: 0,
        })?;
    }

    let config = config::configuration();
    if per_pane.config_generation != config.generation() {
        per_pane.config_generation = config.generation();
        // If the config changed, it may have changed colors
        // in the palette that we need to push down, so we
        // synthesize a palette change notification to let
        // the client know
        per_pane.notifications.push(Alert::PaletteChanged);
        per_pane.sent_initial_palette = true;
    }

    if !per_pane.sent_initial_palette {
        per_pane.notifications.push(Alert::PaletteChanged);
        per_pane.sent_initial_palette = true;
    }
    for alert in per_pane.notifications.drain(..) {
        match alert {
            Alert::PaletteChanged => {
                sender.send(DecodedPdu {
                    pdu: Pdu::SetPalette(SetPalette {
                        pane_id: pane.pane_id(),
                        palette: pane.palette(),
                    }),
                    serial: 0,
                })?;
            }
            alert => {
                sender.send(DecodedPdu {
                    pdu: Pdu::NotifyAlert(NotifyAlert {
                        pane_id: pane.pane_id(),
                        alert,
                    }),
                    serial: 0,
                })?;
            }
        }
    }
    Ok(())
}
