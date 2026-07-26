use super::*;
use mux::pane::CachePolicy;
use std::cmp::Ordering;
use std::sync::Arc;
use termwiz::cell::SemanticType;
use termwiz_funcs::lines_to_escapes;
use url_funcs::Url;
use wezterm_term::{SemanticZone, StableRowIndex};

#[derive(Clone, Copy, Debug)]
pub struct MuxPane(pub PaneId);

impl MuxPane {
    pub fn resolve<'a>(&self, mux: &'a Arc<Mux>) -> anyhow::Result<Arc<dyn Pane>> {
        mux.get_pane(self.0)
            .ok_or_else(|| anyhow::anyhow!(format!("pane id {} not found in mux", self.0)))
    }

    fn get_text_from_semantic_zone(&self, zone: SemanticZone) -> anyhow::Result<String> {
        let mux = get_mux()?;
        let pane = self.resolve(&mux)?;

        let mut last_was_wrapped = false;
        let first_row = zone.start_y;
        let last_row = zone.end_y;

        fn cols_for_row(zone: &SemanticZone, row: StableRowIndex) -> std::ops::Range<usize> {
            if row < zone.start_y || row > zone.end_y {
                0..0
            } else if zone.start_y == zone.end_y {
                // A single line zone
                if zone.start_x <= zone.end_x {
                    zone.start_x..zone.end_x.saturating_add(1)
                } else {
                    zone.end_x..zone.start_x.saturating_add(1)
                }
            } else if row == zone.end_y {
                // last line of multi-line
                0..zone.end_x.saturating_add(1)
            } else if row == zone.start_y {
                // first line of multi-line
                zone.start_x..usize::max_value()
            } else {
                // some "middle" line of multi-line
                0..usize::max_value()
            }
        }

        let mut s = String::new();
        for line in pane.get_logical_lines(zone.start_y..zone.end_y + 1) {
            if !s.is_empty() && !last_was_wrapped {
                s.push('\n');
            }
            let last_idx = line.physical_lines.len().saturating_sub(1);
            for (idx, phys) in line.physical_lines.iter().enumerate() {
                let this_row = line.first_row + idx as StableRowIndex;
                if this_row >= first_row && this_row <= last_row {
                    let last_phys_idx = phys.len().saturating_sub(1);

                    let cols = cols_for_row(&zone, this_row);
                    let last_col_idx = cols.end.saturating_sub(1).min(last_phys_idx);
                    let col_span = phys.columns_as_str(cols);
                    // Only trim trailing whitespace if we are the last line
                    // in a wrapped sequence
                    if idx == last_idx {
                        s.push_str(col_span.trim_end());
                    } else {
                        s.push_str(&col_span);
                    }

                    last_was_wrapped = last_col_idx == last_phys_idx
                        && phys
                            .get_cell(last_col_idx)
                            .map(|c| c.attrs().wrapped())
                            .unwrap_or(false);
                }
            }
        }

        Ok(s)
    }
}

// ---------------------------------------------------------------------------
// L4d: rhai port of the `MuxPane` UserData above (see
// docs/plans/2026-07-23-lua-rhai-migration.md). Runs in parallel with the mlua
// path; does not replace or touch `impl UserData for MuxPane`.
//
// `split`/`move_to_new_tab`/`move_to_new_window` are async on the mlua side;
// mirroring the same async -> sync adaptation used throughout this port,
// they are driven to completion via `smol::block_on` here.
//
// A few return-shape adaptations, each documented at its call site below:
//   * `get_progress` uses `rhai::serde::to_dynamic` (mirroring the mlua
//     path's `lua.to_value(&progress)`, i.e. `LuaSerdeExt`) since `Progress`
//     (defined in the `term` crate) derives `serde::Serialize` but not
//     `FromDynamic`/`ToDynamic`.
//   * `get_current_working_dir` returns a plain rhai `String` (the URL's
//     string form) rather than a `Url` custom type, since `url-funcs` (the
//     crate that owns `Url`) has no rhai registration of its own yet; this
//     keeps `mux-lua`'s rhai port self-contained without reaching into
//     another crate's not-yet-ported internals.
//   * `get_metadata` uses the same `wezterm_dynamic::Value ->
//     rhai::Dynamic` bridge (`config::rhai_value::dynamic_to_rhai_dynamic`)
//     as everywhere else in this port, since `Pane::get_metadata` already
//     returns a `wezterm_dynamic::Value` directly (not a typed struct).
//   * `get_user_vars` (`HashMap<String, String>`) -> rhai `Map`.
//   * `get_semantic_zones`/`get_semantic_zone_at`/`get_foreground_process_info`
//     go through `config::rhai_value::dynamic_to_rhai_dynamic` on their
//     `ToDynamic` output, since `SemanticZone`/`LocalProcessInfo` already
//     derive `FromDynamic`/`ToDynamic` (used by the mlua path's own
//     `to_lua`/`from_lua` helpers).
pub fn register_rhai(engine: &mut rhai::Engine) -> anyhow::Result<()> {
    engine.register_type_with_name::<MuxPane>("MuxPane");

    engine.register_fn("to_string", |this: &mut MuxPane| -> String {
        format!("MuxPane(pane_id:{}, pid:{})", this.0, unsafe {
            libc::getpid()
        })
    });
    engine.register_fn("pane_id", |this: &mut MuxPane| -> rhai::INT {
        this.0 as rhai::INT
    });

    engine.register_fn(
        "split",
        |this: &mut MuxPane, args: rhai::Map| -> Result<MuxPane, Box<rhai::EvalAltResult>> {
            let args = SplitPane::try_from(rhai::Dynamic::from_map(args))
                .map_err(|err| -> Box<rhai::EvalAltResult> { format!("split: {err}").into() })?;
            smol::block_on(args.run(this))
                .map_err(|err| -> Box<rhai::EvalAltResult> { format!("{err:#}").into() })
        },
    );
    // Zero-arg overload: `pane.split()` with defaults.
    engine.register_fn(
        "split",
        |this: &mut MuxPane| -> Result<MuxPane, Box<rhai::EvalAltResult>> {
            smol::block_on(SplitPane::default().run(this))
                .map_err(|err| -> Box<rhai::EvalAltResult> { format!("{err:#}").into() })
        },
    );

    engine.register_fn(
        "send_paste",
        |this: &mut MuxPane, text: String| -> Result<(), Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let pane = resolve_rhai(this, &mux)?;
            pane.send_paste(&text)
                .map_err(|e| -> Box<rhai::EvalAltResult> { format!("{:#}", e).into() })
        },
    );
    // An alias of send-paste for backwards compatibility with prior releases
    // when there was a separate Gui-level PaneObject (mirroring the mlua
    // path's own `paste` alias).
    engine.register_fn(
        "paste",
        |this: &mut MuxPane, text: String| -> Result<(), Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let pane = resolve_rhai(this, &mux)?;
            pane.send_paste(&text)
                .map_err(|e| -> Box<rhai::EvalAltResult> { format!("{:#}", e).into() })
        },
    );

    engine.register_fn(
        "send_text",
        |this: &mut MuxPane, text: String| -> Result<(), Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let pane = resolve_rhai(this, &mux)?;
            let result = pane
                .writer()
                .write_all(text.as_bytes())
                .map_err(|e| -> Box<rhai::EvalAltResult> { format!("{:#}", e).into() });
            result
        },
    );

    engine.register_fn(
        "window",
        |this: &mut MuxPane| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            Ok(
                match mux
                    .resolve_pane_id(this.0)
                    .map(|(_domain_id, window_id, _tab_id)| MuxWindow(window_id))
                {
                    Some(window) => rhai::Dynamic::from(window),
                    None => rhai::Dynamic::UNIT,
                },
            )
        },
    );
    engine.register_fn(
        "tab",
        |this: &mut MuxPane| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            Ok(
                match mux
                    .resolve_pane_id(this.0)
                    .map(|(_domain_id, _window_id, tab_id)| MuxTab(tab_id))
                {
                    Some(tab) => rhai::Dynamic::from(tab),
                    None => rhai::Dynamic::UNIT,
                },
            )
        },
    );

    // For backwards compatibility with prior releases when there was a
    // separate Gui-level PaneObject (mirroring the mlua path's own
    // `mux_pane` method).
    engine.register_fn("mux_pane", |this: &mut MuxPane| -> MuxPane { *this });

    engine.register_fn(
        "get_title",
        |this: &mut MuxPane| -> Result<String, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let pane = resolve_rhai(this, &mux)?;
            Ok(pane.get_title())
        },
    );

    engine.register_fn(
        "get_progress",
        |this: &mut MuxPane| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let pane = resolve_rhai(this, &mux)?;
            let progress = pane.get_progress();
            rhai::serde::to_dynamic(&progress).map_err(|err| -> Box<rhai::EvalAltResult> {
                format!("get_progress: {err}").into()
            })
        },
    );

    engine.register_fn(
        "get_current_working_dir",
        |this: &mut MuxPane| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let pane = resolve_rhai(this, &mux)?;
            Ok(
                match pane.get_current_working_dir(CachePolicy::FetchImmediate) {
                    Some(url) => rhai::Dynamic::from(url.to_string()),
                    None => rhai::Dynamic::UNIT,
                },
            )
        },
    );

    engine.register_fn(
        "get_metadata",
        |this: &mut MuxPane| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let pane = resolve_rhai(this, &mux)?;
            let value = pane.get_metadata();
            Ok(config::rhai_value::dynamic_to_rhai_dynamic(&value))
        },
    );

    engine.register_fn(
        "get_foreground_process_name",
        |this: &mut MuxPane| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let pane = resolve_rhai(this, &mux)?;
            Ok(
                match pane.get_foreground_process_name(CachePolicy::FetchImmediate) {
                    Some(name) => rhai::Dynamic::from(name),
                    None => rhai::Dynamic::UNIT,
                },
            )
        },
    );

    engine.register_fn(
        "get_foreground_process_info",
        |this: &mut MuxPane| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let pane = resolve_rhai(this, &mux)?;
            Ok(
                match pane.get_foreground_process_info(CachePolicy::AllowStale) {
                    Some(info) => config::rhai_value::dynamic_to_rhai_dynamic(&info.to_dynamic()),
                    None => rhai::Dynamic::UNIT,
                },
            )
        },
    );

    engine.register_fn(
        "get_cursor_position",
        |this: &mut MuxPane| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let pane = resolve_rhai(this, &mux)?;
            Ok(config::rhai_value::dynamic_to_rhai_dynamic(
                &pane.get_cursor_position().to_dynamic(),
            ))
        },
    );

    engine.register_fn(
        "get_dimensions",
        |this: &mut MuxPane| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let pane = resolve_rhai(this, &mux)?;
            Ok(config::rhai_value::dynamic_to_rhai_dynamic(
                &pane.get_dimensions().to_dynamic(),
            ))
        },
    );

    engine.register_fn(
        "get_user_vars",
        |this: &mut MuxPane| -> Result<rhai::Map, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let pane = resolve_rhai(this, &mux)?;
            let mut map = rhai::Map::new();
            for (k, v) in pane.copy_user_vars().into_iter() {
                map.insert(k.into(), rhai::Dynamic::from(v));
            }
            Ok(map)
        },
    );

    engine.register_fn(
        "has_unseen_output",
        |this: &mut MuxPane| -> Result<bool, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let pane = resolve_rhai(this, &mux)?;
            Ok(pane.has_unseen_output())
        },
    );

    engine.register_fn(
        "is_alt_screen_active",
        |this: &mut MuxPane| -> Result<bool, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let pane = resolve_rhai(this, &mux)?;
            Ok(pane.is_alt_screen_active())
        },
    );

    engine.register_fn(
        "get_lines_as_text",
        |this: &mut MuxPane, nlines: rhai::INT| -> Result<String, Box<rhai::EvalAltResult>> {
            get_lines_as_text_rhai(this, Some(nlines))
        },
    );
    // Zero-arg overload: returns the lines from the viewport as plain text
    // (no escape sequences), mirroring the mlua path's `Option<usize>`.
    engine.register_fn(
        "get_lines_as_text",
        |this: &mut MuxPane| -> Result<String, Box<rhai::EvalAltResult>> {
            get_lines_as_text_rhai(this, None)
        },
    );

    engine.register_fn(
        "get_lines_as_escapes",
        |this: &mut MuxPane, nlines: rhai::INT| -> Result<String, Box<rhai::EvalAltResult>> {
            get_lines_as_escapes_rhai(this, Some(nlines))
        },
    );
    engine.register_fn(
        "get_lines_as_escapes",
        |this: &mut MuxPane| -> Result<String, Box<rhai::EvalAltResult>> {
            get_lines_as_escapes_rhai(this, None)
        },
    );

    engine.register_fn(
        "get_logical_lines_as_text",
        |this: &mut MuxPane, nlines: rhai::INT| -> Result<String, Box<rhai::EvalAltResult>> {
            get_logical_lines_as_text_rhai(this, Some(nlines))
        },
    );
    engine.register_fn(
        "get_logical_lines_as_text",
        |this: &mut MuxPane| -> Result<String, Box<rhai::EvalAltResult>> {
            get_logical_lines_as_text_rhai(this, None)
        },
    );

    engine.register_fn(
        "get_domain_name",
        |this: &mut MuxPane| -> Result<String, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let pane = resolve_rhai(this, &mux)?;
            let mut name = None;
            if let Some(mux) = Mux::try_get() {
                let domain_id = pane.domain_id();
                name = mux
                    .get_domain(domain_id)
                    .map(|dom| dom.domain_name().to_string());
            }
            Ok(name.unwrap_or_default())
        },
    );

    engine.register_fn(
        "inject_output",
        |this: &mut MuxPane, text: String| -> Result<(), Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let pane = resolve_rhai(this, &mux)?;

            let mut parser = termwiz::escape::parser::Parser::new();
            let mut actions = vec![];
            parser.parse(text.as_bytes(), |action| actions.push(action));

            pane.perform_actions(actions);
            Ok(())
        },
    );

    engine.register_fn(
        "get_semantic_zones",
        |this: &mut MuxPane,
         of_type: rhai::Dynamic|
         -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
            let of_type: Option<SemanticType> = if of_type.is_unit() {
                None
            } else {
                Some(
                    config::rhai_value::rhai_dynamic_to_value(&of_type).map_err(
                        |err| -> Box<rhai::EvalAltResult> {
                            format!("get_semantic_zones: {err}").into()
                        },
                    )?,
                )
            };
            get_semantic_zones_rhai(this, of_type)
        },
    );
    // Zero-arg overload: no `of_type` filter.
    engine.register_fn(
        "get_semantic_zones",
        |this: &mut MuxPane| -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
            get_semantic_zones_rhai(this, None)
        },
    );

    engine.register_fn(
        "get_semantic_zone_at",
        |this: &mut MuxPane,
         x: rhai::INT,
         y: rhai::INT|
         -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let pane = resolve_rhai(this, &mux)?;
            let x = usize::try_from(x).map_err(|_| -> Box<rhai::EvalAltResult> {
                format!("get_semantic_zone_at: invalid x {x}").into()
            })?;
            let y = y as StableRowIndex;

            let zones = pane.get_semantic_zones().unwrap_or_else(|_| vec![]);

            match zones.binary_search_by(|zone| find_zone(x, y, zone)) {
                Ok(idx) => Ok(config::rhai_value::dynamic_to_rhai_dynamic(
                    &zones[idx].to_dynamic(),
                )),
                Err(_) => Ok(rhai::Dynamic::UNIT),
            }
        },
    );

    engine.register_fn(
        "get_text_from_semantic_zone",
        |this: &mut MuxPane, zone: rhai::Dynamic| -> Result<String, Box<rhai::EvalAltResult>> {
            let zone: SemanticZone = config::rhai_value::rhai_dynamic_to_value(&zone).map_err(
                |err| -> Box<rhai::EvalAltResult> {
                    format!("get_text_from_semantic_zone: {err}").into()
                },
            )?;
            get_text_from_semantic_zone_rhai(this, zone)
        },
    );

    engine.register_fn(
        "get_text_from_region",
        |this: &mut MuxPane,
         start_x: rhai::INT,
         start_y: rhai::INT,
         end_x: rhai::INT,
         end_y: rhai::INT|
         -> Result<String, Box<rhai::EvalAltResult>> {
            let start_x = usize::try_from(start_x).map_err(|_| -> Box<rhai::EvalAltResult> {
                format!("get_text_from_region: invalid start_x {start_x}").into()
            })?;
            let end_x = usize::try_from(end_x).map_err(|_| -> Box<rhai::EvalAltResult> {
                format!("get_text_from_region: invalid end_x {end_x}").into()
            })?;
            let zone = SemanticZone {
                start_x,
                start_y: start_y as StableRowIndex,
                end_x,
                end_y: end_y as StableRowIndex,
                // semantic_type is not used by get_text_from_semantic_zone
                semantic_type: SemanticType::Output,
            };
            get_text_from_semantic_zone_rhai(this, zone)
        },
    );

    engine.register_fn(
        "move_to_new_tab",
        |this: &mut MuxPane| -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
            let mux = Mux::get();
            let (_domain, window_id, _tab) =
                mux.resolve_pane_id(this.0)
                    .ok_or_else(|| -> Box<rhai::EvalAltResult> {
                        format!("pane {} not found", this.0).into()
                    })?;
            let (tab, window) = smol::block_on(mux.move_pane_to_new_tab(
                this.0,
                Some(window_id),
                None,
            ))
            .map_err(|e| -> Box<rhai::EvalAltResult> { format!("{:#?}", e).into() })?;

            Ok(vec![
                rhai::Dynamic::from(MuxTab(tab.tab_id())),
                rhai::Dynamic::from(MuxWindow(window)),
            ])
        },
    );

    engine.register_fn(
        "move_to_new_window",
        |this: &mut MuxPane,
         workspace: String|
         -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
            move_to_new_window_rhai(this, Some(workspace))
        },
    );
    // Zero-arg overload: no explicit workspace (mirroring the mlua path's
    // `Option<String>`).
    engine.register_fn(
        "move_to_new_window",
        |this: &mut MuxPane| -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
            move_to_new_window_rhai(this, None)
        },
    );

    engine.register_fn(
        "activate",
        |this: &mut MuxPane| -> Result<(), Box<rhai::EvalAltResult>> {
            let mux = Mux::get();
            let pane = resolve_rhai(this, &mux)?;
            let (_domain_id, window_id, tab_id) =
                mux.resolve_pane_id(this.0)
                    .ok_or_else(|| -> Box<rhai::EvalAltResult> {
                        format!("pane {} not found", this.0).into()
                    })?;
            {
                let mut window = mux.get_window_mut(window_id).ok_or_else(
                    || -> Box<rhai::EvalAltResult> {
                        format!("window {window_id} not found").into()
                    },
                )?;
                let tab_idx = window.idx_by_id(tab_id).ok_or_else(
                    || -> Box<rhai::EvalAltResult> {
                        format!("tab {tab_id} isn't really in window {window_id}!?").into()
                    },
                )?;
                window.save_and_then_set_active(tab_idx);
            }
            let tab = mux
                .get_tab(tab_id)
                .ok_or_else(|| -> Box<rhai::EvalAltResult> {
                    format!("tab {tab_id} not found").into()
                })?;
            tab.set_active_pane(&pane);
            Ok(())
        },
    );

    engine.register_fn(
        "get_tty_name",
        |this: &mut MuxPane| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            let mux = Mux::get();
            let pane = resolve_rhai(this, &mux)?;
            Ok(match pane.tty_name() {
                Some(name) => rhai::Dynamic::from(name),
                None => rhai::Dynamic::UNIT,
            })
        },
    );

    Ok(())
}

/// rhai-flavored equivalent of `get_mux()` (defined in `lib.rs` for the mlua
/// path).
pub(crate) fn get_mux_rhai() -> Result<Arc<Mux>, Box<rhai::EvalAltResult>> {
    Mux::try_get().ok_or_else(|| "cannot get Mux!?".into())
}

/// rhai-flavored equivalent of `MuxPane::resolve` above, also used by
/// `lib.rs`'s `mux::get_pane` binding (which lives outside this module).
pub(crate) fn resolve_rhai(
    this: &MuxPane,
    mux: &Arc<Mux>,
) -> Result<Arc<dyn Pane>, Box<rhai::EvalAltResult>> {
    mux.get_pane(this.0)
        .ok_or_else(|| format!("pane id {} not found in mux", this.0).into())
}

fn find_zone(x: usize, y: StableRowIndex, zone: &SemanticZone) -> Ordering {
    match zone.start_y.cmp(&y) {
        Ordering::Greater => return Ordering::Greater,
        // If the zone starts on the same line then check that the
        // x position is within bounds
        Ordering::Equal => match zone.start_x.cmp(&x) {
            Ordering::Greater => return Ordering::Greater,
            Ordering::Equal | Ordering::Less => {}
        },
        Ordering::Less => {}
    }
    match zone.end_y.cmp(&y) {
        Ordering::Less => Ordering::Less,
        // If the zone ends on the same line then check that the
        // x position is within bounds
        Ordering::Equal => match zone.end_x.cmp(&x) {
            Ordering::Less => Ordering::Less,
            Ordering::Equal | Ordering::Greater => Ordering::Equal,
        },
        Ordering::Greater => Ordering::Equal,
    }
}

fn get_lines_as_text_rhai(
    this: &MuxPane,
    nlines: Option<rhai::INT>,
) -> Result<String, Box<rhai::EvalAltResult>> {
    let mux = get_mux_rhai()?;
    let pane = resolve_rhai(this, &mux)?;
    let dims = pane.get_dimensions();
    let nlines = nlines_from_rhai(nlines, dims.viewport_rows)?;
    let bottom_row = dims.physical_top + dims.viewport_rows as isize;
    let top_row = bottom_row.saturating_sub(nlines as isize);
    let (_first_row, lines) = pane.get_lines(top_row..bottom_row);
    let mut text = String::new();
    for line in lines {
        for cell in line.visible_cells() {
            text.push_str(cell.str());
        }
        let trimmed = text.trim_end().len();
        text.truncate(trimmed);
        text.push('\n');
    }
    let trimmed = text.trim_end().len();
    text.truncate(trimmed);
    Ok(text)
}

fn get_lines_as_escapes_rhai(
    this: &MuxPane,
    nlines: Option<rhai::INT>,
) -> Result<String, Box<rhai::EvalAltResult>> {
    let mux = get_mux_rhai()?;
    let pane = resolve_rhai(this, &mux)?;
    let dims = pane.get_dimensions();
    let nlines = nlines_from_rhai(nlines, dims.viewport_rows)?;
    let bottom_row = dims.physical_top + dims.viewport_rows as isize;
    let top_row = bottom_row.saturating_sub(nlines as isize);
    let (_first_row, lines) = pane.get_lines(top_row..bottom_row);
    lines_to_escapes(lines)
        .map_err(|err| -> Box<rhai::EvalAltResult> { format!("{err:#}").into() })
}

fn get_logical_lines_as_text_rhai(
    this: &MuxPane,
    nlines: Option<rhai::INT>,
) -> Result<String, Box<rhai::EvalAltResult>> {
    let mux = get_mux_rhai()?;
    let pane = resolve_rhai(this, &mux)?;
    let dims = pane.get_dimensions();
    let nlines = nlines_from_rhai(nlines, dims.viewport_rows)?;
    let bottom_row = dims.physical_top + dims.viewport_rows as isize;
    let top_row = bottom_row.saturating_sub(nlines as isize);
    let lines = pane.get_logical_lines(top_row..bottom_row);
    let mut text = String::new();
    for line in lines {
        for cell in line.logical.visible_cells() {
            text.push_str(cell.str());
        }
        let trimmed = text.trim_end().len();
        text.truncate(trimmed);
        text.push('\n');
    }
    let trimmed = text.trim_end().len();
    text.truncate(trimmed);
    Ok(text)
}

fn nlines_from_rhai(
    nlines: Option<rhai::INT>,
    default: usize,
) -> Result<usize, Box<rhai::EvalAltResult>> {
    match nlines {
        Some(n) => usize::try_from(n)
            .map_err(|_| -> Box<rhai::EvalAltResult> { format!("invalid nlines {n}").into() }),
        None => Ok(default),
    }
}

fn get_semantic_zones_rhai(
    this: &MuxPane,
    of_type: Option<SemanticType>,
) -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
    let mux = get_mux_rhai()?;
    let pane = resolve_rhai(this, &mux)?;

    let mut zones = pane
        .get_semantic_zones()
        .map_err(|e| -> Box<rhai::EvalAltResult> { format!("{:#}", e).into() })?;

    if let Some(of_type) = of_type {
        zones.retain(|zone| zone.semantic_type == of_type);
    }

    Ok(zones
        .into_iter()
        .map(|zone| config::rhai_value::dynamic_to_rhai_dynamic(&zone.to_dynamic()))
        .collect())
}

fn get_text_from_semantic_zone_rhai(
    this: &MuxPane,
    zone: SemanticZone,
) -> Result<String, Box<rhai::EvalAltResult>> {
    this.get_text_from_semantic_zone(zone)
        .map_err(|err| -> Box<rhai::EvalAltResult> { format!("{err:#}").into() })
}

fn move_to_new_window_rhai(
    this: &MuxPane,
    workspace: Option<String>,
) -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
    let mux = Mux::get();
    let (tab, window) = smol::block_on(mux.move_pane_to_new_tab(this.0, None, workspace))
        .map_err(|e| -> Box<rhai::EvalAltResult> { format!("{:#?}", e).into() })?;

    Ok(vec![
        rhai::Dynamic::from(MuxTab(tab.tab_id())),
        rhai::Dynamic::from(MuxWindow(window)),
    ])
}

#[derive(Debug, Default, FromDynamic, ToDynamic)]
struct SplitPane {
    #[dynamic(flatten)]
    cmd_builder: CommandBuilderFrag,
    #[dynamic(default = "spawn_tab_default_domain")]
    domain: SpawnTabDomain,
    #[dynamic(default)]
    direction: HandySplitDirection,
    #[dynamic(default)]
    top_level: bool,
    #[dynamic(default = "default_split_size")]
    size: f32,
}
impl_rhai_conversion_dynamic!(SplitPane);

fn default_split_size() -> f32 {
    0.5
}

impl SplitPane {
    async fn run(&self, pane: &MuxPane) -> anyhow::Result<MuxPane> {
        let (command, command_dir) = self.cmd_builder.to_command_builder();
        let source = SplitSource::Spawn {
            command,
            command_dir,
        };

        let size = if self.size == 0.0 {
            SplitSize::Percent(50)
        } else if self.size < 1.0 {
            SplitSize::Percent((self.size * 100.).floor() as u8)
        } else {
            SplitSize::Cells(self.size as usize)
        };

        let direction = match self.direction {
            HandySplitDirection::Right | HandySplitDirection::Left => SplitDirection::Horizontal,
            HandySplitDirection::Top | HandySplitDirection::Bottom => SplitDirection::Vertical,
        };

        let request = SplitRequest {
            direction,
            target_is_second: match self.direction {
                HandySplitDirection::Top | HandySplitDirection::Left => false,
                HandySplitDirection::Bottom | HandySplitDirection::Right => true,
            },
            top_level: self.top_level,
            size,
        };

        let mux = get_mux()?;
        let (pane, _size) = mux
            .split_pane(pane.0, request, source, self.domain.clone())
            .await
            .map_err(|e| anyhow::anyhow!(format!("{:#?}", e)))?;

        Ok(MuxPane(pane.pane_id()))
    }
}
