use hematite_ui_state::{ShellState, UtilityTab};
use slint::{ModelRc, SharedString, VecModel};
use std::{cell::RefCell, rc::Rc};

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let window = MainWindow::new()?;
    let state = Rc::new(RefCell::new(ShellState::with_sample_workspace()));

    refresh_window(&window, &state.borrow());
    wire_callbacks(&window, state);

    window.run()
}

fn wire_callbacks(window: &MainWindow, state: Rc<RefCell<ShellState>>) {
    let weak_window = window.as_weak();
    let state_for_open = Rc::clone(&state);
    window.on_open_sample_document(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };
        state_for_open.borrow_mut().open_document(
            "src/main.rs",
            "fn main() {\n    println!(\"hello from Hematite Slint\");\n}\n",
        );
        refresh_window(&window, &state_for_open.borrow());
    });

    let weak_window = window.as_weak();
    let state_for_save = Rc::clone(&state);
    window.on_save_active_document(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };
        state_for_save.borrow_mut().save_active_document();
        refresh_chrome(&window, &state_for_save.borrow());
    });

    let weak_window = window.as_weak();
    let state_for_edit = Rc::clone(&state);
    window.on_editor_edited(move |text| {
        let Some(window) = weak_window.upgrade() else {
            return;
        };
        state_for_edit
            .borrow_mut()
            .edit_active_document(text.to_string());
        refresh_chrome(&window, &state_for_edit.borrow());
    });

    let weak_window = window.as_weak();
    let state_for_utility = Rc::clone(&state);
    window.on_select_utility_tab(move |label| {
        let Some(window) = weak_window.upgrade() else {
            return;
        };
        state_for_utility
            .borrow_mut()
            .select_utility_tab(utility_tab_from_label(label.as_str()));
        refresh_chrome(&window, &state_for_utility.borrow());
    });
}

fn refresh_window(window: &MainWindow, state: &ShellState) {
    refresh_chrome(window, state);
    window.set_editor_text(state.active_content().into());
}

fn refresh_chrome(window: &MainWindow, state: &ShellState) {
    window.set_workspace_title(state.workspace_title().into());
    window.set_status_text(state.status_message().into());
    window.set_explorer_entries(model_from_strings(state.explorer_labels()));
    window.set_open_tabs(model_from_strings(state.tab_labels()));
}

fn model_from_strings(items: Vec<String>) -> ModelRc<SharedString> {
    let rows = items
        .into_iter()
        .map(SharedString::from)
        .collect::<Vec<SharedString>>();
    ModelRc::new(Rc::new(VecModel::from(rows)))
}

fn utility_tab_from_label(label: &str) -> UtilityTab {
    match label {
        "Tooling" => UtilityTab::Tooling,
        "Outline" => UtilityTab::Outline,
        _ => UtilityTab::Agents,
    }
}
