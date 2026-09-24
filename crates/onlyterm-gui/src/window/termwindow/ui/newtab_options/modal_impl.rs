use super::*;

impl Modal for NewTabOptions {
    fn perform_assignment(
        &self,
        _assignment: &KeyAssignment,
        _term_window: &mut TermWindow,
    ) -> bool {
        false
    }

    fn mouse_event(&self, _event: MouseEvent, _term_window: &mut TermWindow) -> anyhow::Result<()> {
        Ok(())
    }

    fn key_down(
        &self,
        key: KeyCode,
        mods: KeyModifiers,
        term_window: &mut TermWindow,
    ) -> anyhow::Result<bool> {
        match (key, mods) {
            (KeyCode::Escape, KeyModifiers::NONE) => {
                self.dismiss(term_window);
            }
            (KeyCode::Tab, KeyModifiers::NONE) => {
                self.move_focus(1);
                term_window.invalidate_modal();
            }
            (KeyCode::Tab, KeyModifiers::SHIFT) => {
                self.move_focus(-1);
                term_window.invalidate_modal();
            }
            (KeyCode::Char(' '), KeyModifiers::NONE) | (KeyCode::Enter, KeyModifiers::NONE) => {
                match self.select_focused_impl() {
                    Some(request) => {
                        term_window.cancel_modal();
                        execute_new_tab_run_request(term_window, request);
                    }
                    None => term_window.invalidate_modal(),
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn computed_element(
        &self,
        term_window: &mut TermWindow,
    ) -> anyhow::Result<Ref<'_, [ComputedElement]>> {
        if self.element.borrow().is_none() {
            let element = Self::compute(
                term_window,
                &self.shells,
                *self.selected_shell.borrow(),
                *self.selected_elevation.borrow(),
                *self.selected_priority.borrow(),
                *self.focus.borrow(),
            )?;
            self.element.borrow_mut().replace(element);
        }
        Ok(Ref::map(self.element.borrow(), |v| {
            v.as_ref().unwrap().as_slice()
        }))
    }

    fn reconfigure(&self, _term_window: &mut TermWindow) {
        self.element.borrow_mut().take();
    }
}
