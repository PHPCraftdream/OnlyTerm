use super::*;

impl TermWindow {
    pub(super) fn show_input_selector(
        &mut self,
        args: &onlyterm_config::keyassignment::InputSelector,
    ) {
        let mux = Mux::get();
        let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
            Some(tab) => tab,
            None => return,
        };

        // Ignore any current overlay: we're going to cancel it out below
        // and we don't want this new one to reference that cancelled pane
        let pane = match self.get_active_pane_no_overlay() {
            Some(pane) => pane,
            None => return,
        };

        let args = args.clone();

        let gui_win = GuiWin::new(self);
        let pane = MuxPane(pane.pane_id());

        let (overlay, future) = match start_overlay(self, &tab, move |_tab_id, term| {
            crate::overlay::selector::selector(term, args, gui_win, pane)
        }) {
            Ok(res) => res,
            Err(err) => {
                log::error!("Failed to show selector overlay: {err:#}");
                return;
            }
        };
        self.assign_overlay(tab.tab_id(), overlay);
        onlyterm_promise::spawn::spawn(future).detach();
    }

    /// Opens the graphical title editor for the captured active tab.
    pub(in crate::termwindow) fn rename_current_tab(&mut self) {
        if let Some(menu) = crate::termwindow::rename_tab_menu::RenameTabMenu::new(self) {
            self.set_modal(Rc::new(menu));
        }
    }

    pub(super) fn show_prompt_input_line(&mut self, args: &PromptInputLine) {
        let mux = Mux::get();
        let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
            Some(tab) => tab,
            None => return,
        };

        let pane = match self.get_active_pane_or_overlay() {
            Some(pane) => pane,
            None => return,
        };

        let args = args.clone();

        let gui_win = GuiWin::new(self);
        let pane = MuxPane(pane.pane_id());

        let (overlay, future) = match start_overlay(self, &tab, move |_tab_id, term| {
            crate::overlay::prompt::show_line_prompt_overlay(term, args, gui_win, pane)
        }) {
            Ok(res) => res,
            Err(err) => {
                log::error!("Failed to show prompt input line overlay: {err:#}");
                return;
            }
        };
        self.assign_overlay(tab.tab_id(), overlay);
        onlyterm_promise::spawn::spawn(future).detach();
    }

    pub(super) fn show_confirmation(&mut self, args: &Confirmation) {
        let mux = Mux::get();
        let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
            Some(tab) => tab,
            None => return,
        };

        let pane = match self.get_active_pane_or_overlay() {
            Some(pane) => pane,
            None => return,
        };

        let args = args.clone();

        let gui_win = GuiWin::new(self);
        let pane = MuxPane(pane.pane_id());

        let (overlay, future) = match start_overlay(self, &tab, move |_tab_id, term| {
            crate::overlay::confirm::show_confirmation_overlay(term, args, gui_win, pane)
        }) {
            Ok(res) => res,
            Err(err) => {
                log::error!("Failed to show confirmation overlay: {err:#}");
                return;
            }
        };
        self.assign_overlay(tab.tab_id(), overlay);
        onlyterm_promise::spawn::spawn(future).detach();
    }

    pub(super) fn show_version_overlay(&mut self) {
        let mux = Mux::get();
        let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
            Some(tab) => tab,
            None => return,
        };

        let gui_win = GuiWin::new(self);

        let (overlay, future) = match start_overlay(self, &tab, move |_tab_id, term| {
            crate::overlay::show_version_overlay(term, gui_win)
        }) {
            Ok(res) => res,
            Err(err) => {
                log::error!("Failed to show version overlay: {err:#}");
                return;
            }
        };
        self.assign_overlay(tab.tab_id(), overlay);
        onlyterm_promise::spawn::spawn(future).detach();
    }

    pub(super) fn show_debug_overlay(&mut self) {
        let mux = Mux::get();
        let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
            Some(tab) => tab,
            None => return,
        };

        let gui_win = GuiWin::new(self);

        let renderer_info = self
            .renderer_info
            .as_deref()
            .unwrap_or("Unknown")
            .to_string();
        let connection_info = self.connection_name.clone();

        let (overlay, future) = match start_overlay(self, &tab, move |_tab_id, term| {
            crate::overlay::show_debug_overlay(term, gui_win, renderer_info, connection_info)
        }) {
            Ok(res) => res,
            Err(err) => {
                log::error!("Failed to show debug overlay: {err:#}");
                return;
            }
        };
        self.assign_overlay(tab.tab_id(), overlay);
        onlyterm_promise::spawn::spawn(future).detach();
    }

    pub(super) fn show_launcher_impl(
        &mut self,
        args: LauncherActionArgs,
        initial_choice_idx: usize,
    ) {
        let mux_window_id = self.mux_window_id;
        let window = self.window.as_ref().unwrap().clone();

        let mux = Mux::get();
        let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
            Some(tab) => tab,
            None => return,
        };

        let pane = match self.get_active_pane_or_overlay() {
            Some(pane) => pane,
            None => return,
        };

        let domain_id_of_current_pane = tab
            .get_active_pane()
            .expect("tab has no panes!")
            .domain_id();
        let pane_id = pane.pane_id();
        let tab_id = tab.tab_id();
        let title = args.title.unwrap();
        let flags = args.flags;
        let help_text = args.help_text.unwrap_or(
            "Select an item and press Enter=launch  \
             Esc=cancel  /=filter"
                .to_string(),
        );
        let fuzzy_help_text = args
            .fuzzy_help_text
            .unwrap_or("Fuzzy matching: ".to_string());

        let config = &self.config;
        let alphabet = args.alphabet.unwrap_or(config.launcher_alphabet.clone());

        onlyterm_promise::spawn::spawn(async move {
            let args = LauncherArgs::new(
                &title,
                flags,
                mux_window_id,
                pane_id,
                domain_id_of_current_pane,
                &help_text,
                &fuzzy_help_text,
                &alphabet,
            )
            .await;

            let win = window.clone();
            win.notify(TermWindowNotif::Apply(Box::new(move |term_window| {
                let mux = Mux::get();
                if let Some(tab) = mux.get_tab(tab_id) {
                    let window = window.clone();
                    let (overlay, future) =
                        match start_overlay(term_window, &tab, move |_tab_id, term| {
                            launcher(args, term, window, initial_choice_idx)
                        }) {
                            Ok(res) => res,
                            Err(err) => {
                                log::error!("Failed to show launcher overlay: {err:#}");
                                return;
                            }
                        };

                    term_window.assign_overlay(tab_id, overlay);
                    onlyterm_promise::spawn::spawn(future).detach();
                }
            })));
        })
        .detach();
    }
}
