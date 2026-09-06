use super::*;

impl<'a> Performer<'a> {
    pub(super) fn osc_dispatch(&mut self, osc: OperatingSystemCommand) {
        self.pop_tmux_title_state();
        self.flush_print();
        match osc {
            OperatingSystemCommand::SetIconNameSun(_)
            | OperatingSystemCommand::SetIconName(_)
            | OperatingSystemCommand::SetIconNameAndWindowTitle(_)
            | OperatingSystemCommand::SetWindowTitleSun(_)
            | OperatingSystemCommand::SetWindowTitle(_)
                if !self.config.allow_process_title_updates() => {}
            OperatingSystemCommand::SetIconNameSun(title)
            | OperatingSystemCommand::SetIconName(title) => {
                if title.is_empty() {
                    self.icon_title = None;
                } else {
                    self.icon_title = Some(title);
                }
                let title = self.icon_title.clone();
                if let Some(handler) = self.alert_handler.as_mut() {
                    handler.alert(Alert::IconTitleChanged(title));
                }
            }
            OperatingSystemCommand::SetIconNameAndWindowTitle(title) => {
                self.icon_title.take();
                self.title = title.clone();
                if let Some(handler) = self.alert_handler.as_mut() {
                    handler.alert(Alert::WindowTitleChanged(title.clone()));
                    handler.alert(Alert::IconTitleChanged(Some(title)));
                }
            }
            OperatingSystemCommand::SetWindowTitleSun(title)
            | OperatingSystemCommand::SetWindowTitle(title) => {
                self.title = title.clone();
                if let Some(handler) = self.alert_handler.as_mut() {
                    handler.alert(Alert::WindowTitleChanged(title));
                }
            }
            OperatingSystemCommand::SetHyperlink(link) => {
                self.set_hyperlink(link);
            }
            OperatingSystemCommand::Unspecified(unspec) => {
                if self.config.log_unknown_escape_sequences() {
                    let mut output = String::new();
                    write!(&mut output, "Unhandled OSC ").ok();

                    for item in unspec {
                        write!(&mut output, " {}", String::from_utf8_lossy(&item)).ok();
                    }
                    log::warn!("{}", output);
                }
            }

            OperatingSystemCommand::ClearSelection(selection) => {
                let selection = selection_to_selection(selection);
                self.set_clipboard_contents(selection, None).ok();
            }
            OperatingSystemCommand::QuerySelection(_) => {}
            OperatingSystemCommand::SetSelection(selection, selection_data) => {
                let selection = selection_to_selection(selection);
                match self.set_clipboard_contents(selection, Some(selection_data)) {
                    Ok(_) => (),
                    Err(err) => error!("failed to set clipboard in response to OSC 52: {:#?}", err),
                }
            }
            OperatingSystemCommand::ITermProprietary(iterm) => match iterm {
                ITermProprietary::RequestCellSize => {
                    let screen = self.screen();
                    let height = screen.physical_rows;
                    let width = screen.physical_cols;

                    let scale = if screen.dpi == 0 {
                        1.0
                    } else {
                        // Since iTerm2 is a macOS specific piece
                        // of software, it uses the macOS default dpi
                        // if 72 for the basis of its scale, regardless
                        // of the host base dpi.
                        screen.dpi as f32 / 72.
                    };
                    let width = (self.pixel_width as f32 / width as f32) / scale;
                    let height = (self.pixel_height as f32 / height as f32) / scale;

                    let response = OperatingSystemCommand::ITermProprietary(
                        ITermProprietary::ReportCellSize {
                            width_pixels: NotNan::new(width).unwrap(),
                            height_pixels: NotNan::new(height).unwrap(),
                            scale: if screen.dpi == 0 {
                                None
                            } else {
                                Some(NotNan::new(scale).unwrap())
                            },
                        },
                    );
                    write!(self.writer, "{}", response).ok();
                    self.writer.flush().ok();
                }
                ITermProprietary::File(image) => self.set_image(*image),
                ITermProprietary::SetUserVar { name, value } => {
                    self.user_vars.insert(name.clone(), value.clone());
                    if let Some(handler) = self.alert_handler.as_mut() {
                        handler.alert(Alert::SetUserVar { name, value });
                    }
                }
                ITermProprietary::UnicodeVersion(ITermUnicodeVersionOp::Set(n)) => {
                    self.unicode_version.version = n;
                }
                ITermProprietary::UnicodeVersion(ITermUnicodeVersionOp::Push(label)) => {
                    let vers = self.unicode_version.clone();
                    self.unicode_version_stack
                        .push(UnicodeVersionStackEntry { vers, label });
                }
                ITermProprietary::UnicodeVersion(ITermUnicodeVersionOp::Pop(None)) => {
                    if let Some(entry) = self.unicode_version_stack.pop() {
                        self.unicode_version = entry.vers;
                    }
                }
                ITermProprietary::UnicodeVersion(ITermUnicodeVersionOp::Pop(Some(label))) => {
                    while let Some(entry) = self.unicode_version_stack.pop() {
                        self.unicode_version = entry.vers;
                        if entry.label.as_deref() == Some(&label) {
                            break;
                        }
                    }
                }
                _ => {
                    if self.config.log_unknown_escape_sequences() {
                        log::warn!("unhandled iterm2: {:?}", iterm);
                    }
                }
            },

            OperatingSystemCommand::FinalTermSemanticPrompt(FinalTermSemanticPrompt::FreshLine) => {
                self.fresh_line();
            }
            OperatingSystemCommand::FinalTermSemanticPrompt(
                FinalTermSemanticPrompt::FreshLineAndStartPrompt { .. },
            ) => {
                self.fresh_line();
                self.pen.set_semantic_type(SemanticType::Prompt);
            }
            OperatingSystemCommand::FinalTermSemanticPrompt(
                FinalTermSemanticPrompt::StartPrompt(_),
            ) => {
                self.pen.set_semantic_type(SemanticType::Prompt);
            }
            OperatingSystemCommand::FinalTermSemanticPrompt(
                FinalTermSemanticPrompt::MarkEndOfCommandWithFreshLine { .. },
            ) => {
                self.fresh_line();
                self.pen.set_semantic_type(SemanticType::Prompt);
            }
            OperatingSystemCommand::FinalTermSemanticPrompt(
                FinalTermSemanticPrompt::MarkEndOfPromptAndStartOfInputUntilNextMarker,
            ) => {
                self.pen.set_semantic_type(SemanticType::Input);
            }
            OperatingSystemCommand::FinalTermSemanticPrompt(
                FinalTermSemanticPrompt::MarkEndOfPromptAndStartOfInputUntilEndOfLine,
            ) => {
                self.pen.set_semantic_type(SemanticType::Input);
                self.clear_semantic_attribute_on_newline = true;
            }
            OperatingSystemCommand::FinalTermSemanticPrompt(
                FinalTermSemanticPrompt::MarkEndOfInputAndStartOfOutput { .. },
            ) => {
                self.pen.set_semantic_type(SemanticType::Output);
            }

            OperatingSystemCommand::FinalTermSemanticPrompt(
                FinalTermSemanticPrompt::CommandStatus { .. },
            ) => {}

            OperatingSystemCommand::SystemNotification(message) => {
                if let Some(handler) = self.alert_handler.as_mut() {
                    handler.alert(Alert::ToastNotification {
                        title: None,
                        body: message,
                        focus: true,
                    });
                } else {
                    log::info!("Application sends SystemNotification: {}", message);
                }
            }
            OperatingSystemCommand::RxvtExtension(params) => {
                if let Some("notify") = params.first().map(String::as_str) {
                    let title = params.get(1);
                    let body = params.get(2);
                    let (title, body) = match (title.cloned(), body.cloned()) {
                        (Some(title), None) => (None, title),
                        (Some(title), Some(body)) => (Some(title), body),
                        _ => {
                            log::warn!("malformed rxvt notify escape: {:?}", params);
                            return;
                        }
                    };
                    if let Some(handler) = self.alert_handler.as_mut() {
                        handler.alert(Alert::ToastNotification {
                            title,
                            body,
                            focus: true,
                        });
                    }
                }
            }
            OperatingSystemCommand::CurrentWorkingDirectory(url) => {
                self.current_dir = Url::parse(&url).ok();
                if let Some(handler) = self.alert_handler.as_mut() {
                    handler.alert(Alert::CurrentWorkingDirectoryChanged);
                }
            }
            OperatingSystemCommand::ChangeColorNumber(specs) => {
                log::trace!("ChangeColorNumber: {:?}", specs);
                for pair in specs {
                    match pair.color {
                        ColorOrQuery::Query => {
                            let response =
                                OperatingSystemCommand::ChangeColorNumber(vec![ChangeColorPair {
                                    palette_index: pair.palette_index,
                                    color: ColorOrQuery::Color(
                                        self.palette().colors.0[pair.palette_index as usize],
                                    ),
                                }]);
                            write!(self.writer, "{}", response).ok();
                            self.writer.flush().ok();
                        }
                        ColorOrQuery::Color(c) => {
                            self.palette_mut().colors.0[pair.palette_index as usize] = c;
                        }
                    }
                }
                self.implicit_palette_reset_if_same_as_configured();
                self.palette_did_change();
            }

            OperatingSystemCommand::ResetColors(colors) => {
                log::trace!("ResetColors: {:?}", colors);
                if colors.is_empty() {
                    // Reset all colors
                    self.palette.take();
                } else {
                    // Reset individual colors
                    if self.palette.is_none() {
                        // Already at the defaults
                    } else {
                        let base = self.config.color_palette();
                        for c in colors {
                            let c = c as usize;
                            self.palette_mut().colors.0[c] = base.colors.0[c];
                        }
                    }
                }
                self.implicit_palette_reset_if_same_as_configured();
                self.palette_did_change();
            }

            OperatingSystemCommand::ChangeDynamicColors(first_color, colors) => {
                log::trace!("ChangeDynamicColors: {:?} {:?}", first_color, colors);
                use onlyterm_escape_parser::osc::DynamicColorNumber;
                for (idx, color) in (first_color as u8..).zip(colors) {
                    let which_color: Option<DynamicColorNumber> = FromPrimitive::from_u8(idx);
                    log::trace!("ChangeDynamicColors item: {:?}", which_color);
                    if let Some(which_color) = which_color {
                        macro_rules! set_or_query {
                            ($name:ident) => {
                                match color {
                                    ColorOrQuery::Query => {
                                        let response = OperatingSystemCommand::ChangeDynamicColors(
                                            which_color,
                                            vec![ColorOrQuery::Color(self.palette().$name.into())],
                                        );
                                        log::trace!("Color Query response {:?}", response);
                                        write!(self.writer, "{}", response).ok();
                                        self.writer.flush().ok();
                                    }
                                    ColorOrQuery::Color(c) => self.palette_mut().$name = c.into(),
                                }
                            };
                        }
                        match which_color {
                            DynamicColorNumber::TextForegroundColor => set_or_query!(foreground),
                            DynamicColorNumber::TextBackgroundColor => set_or_query!(background),
                            DynamicColorNumber::TextCursorColor => {
                                if let ColorOrQuery::Color(c) = color {
                                    // We set the border to the background color; we don't
                                    // have an escape that sets that independently, and this
                                    // way just looks better.
                                    self.palette_mut().cursor_border = c;
                                }
                                set_or_query!(cursor_bg)
                            }
                            DynamicColorNumber::HighlightForegroundColor => {
                                set_or_query!(selection_fg)
                            }
                            DynamicColorNumber::HighlightBackgroundColor => {
                                set_or_query!(selection_bg)
                            }
                            DynamicColorNumber::MouseForegroundColor
                            | DynamicColorNumber::MouseBackgroundColor
                            | DynamicColorNumber::TektronixForegroundColor
                            | DynamicColorNumber::TektronixBackgroundColor
                            | DynamicColorNumber::TektronixCursorColor => {}
                        }
                    }
                }
                self.implicit_palette_reset_if_same_as_configured();
                self.palette_did_change();
            }

            OperatingSystemCommand::ResetDynamicColor(color) => {
                log::trace!("ResetDynamicColor: {:?}", color);
                use onlyterm_escape_parser::osc::DynamicColorNumber;
                let which_color: Option<DynamicColorNumber> = FromPrimitive::from_u8(color as u8);
                if let Some(which_color) = which_color {
                    macro_rules! reset {
                        ($name:ident) => {
                            if self.palette.is_none() {
                                // Already at the defaults
                            } else {
                                let base = self.config.color_palette();
                                self.palette_mut().$name = base.$name;
                            }
                        };
                    }
                    match which_color {
                        DynamicColorNumber::TextForegroundColor => reset!(foreground),
                        DynamicColorNumber::TextBackgroundColor => reset!(background),
                        DynamicColorNumber::TextCursorColor => {
                            reset!(cursor_bg);
                            // Since we set the border to the bg, we consider it reset
                            // by resetting the bg too!
                            reset!(cursor_border);
                        }
                        DynamicColorNumber::HighlightForegroundColor => reset!(selection_fg),
                        DynamicColorNumber::HighlightBackgroundColor => reset!(selection_bg),
                        DynamicColorNumber::MouseForegroundColor
                        | DynamicColorNumber::MouseBackgroundColor
                        | DynamicColorNumber::TektronixForegroundColor
                        | DynamicColorNumber::TektronixBackgroundColor
                        | DynamicColorNumber::TektronixCursorColor => {}
                    }
                }
                self.implicit_palette_reset_if_same_as_configured();
                self.palette_did_change();
            }
            OperatingSystemCommand::ConEmuProgress(prog) => {
                use onlyterm_escape_parser::osc::Progress as TProg;
                let prog = match prog {
                    TProg::None => Progress::None,
                    TProg::SetPercentage(p) => Progress::Percentage(p),
                    TProg::SetError(p) => Progress::Error(p),
                    TProg::SetIndeterminate => Progress::Indeterminate,
                    TProg::Paused => Progress::None,
                };
                if prog != self.progress {
                    self.progress = prog.clone();
                    if let Some(handler) = self.alert_handler.as_mut() {
                        handler.alert(Alert::Progress(prog));
                    }
                }
            }
        }
    }
}

fn selection_to_selection(sel: Selection) -> ClipboardSelection {
    match sel {
        Selection::CLIPBOARD => ClipboardSelection::Clipboard,
        Selection::PRIMARY => ClipboardSelection::PrimarySelection,
        // xterm will use a configurable selection in the NONE case
        Selection::NONE => ClipboardSelection::Clipboard,
        // otherwise we just use clipboard.  Could potentially
        // also use the same fallback configuration as NONE,
        // if/when we add it
        _ => ClipboardSelection::Clipboard,
    }
}
