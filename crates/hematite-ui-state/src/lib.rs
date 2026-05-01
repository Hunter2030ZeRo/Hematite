#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UtilityTab {
    Agents,
    Tooling,
    Outline,
}

impl UtilityTab {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Agents => "Agents",
            Self::Tooling => "Tooling",
            Self::Outline => "Outline",
        }
    }
}

impl Default for UtilityTab {
    fn default() -> Self {
        Self::Agents
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

impl WorkspaceEntry {
    pub fn dir(name: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            is_dir: true,
        }
    }

    pub fn file(name: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            is_dir: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenDocument {
    pub path: String,
    pub title: String,
    pub content: String,
    pub dirty: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SavedDocument {
    pub path: String,
    pub content: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellState {
    workspace_root: Option<String>,
    explorer_entries: Vec<WorkspaceEntry>,
    open_documents: Vec<OpenDocument>,
    active_document: Option<usize>,
    utility_tab: UtilityTab,
    status_message: String,
}

impl Default for ShellState {
    fn default() -> Self {
        Self {
            workspace_root: None,
            explorer_entries: Vec::new(),
            open_documents: Vec::new(),
            active_document: None,
            utility_tab: UtilityTab::default(),
            status_message: "No workspace open.".into(),
        }
    }
}

impl ShellState {
    pub fn with_sample_workspace() -> Self {
        Self {
            workspace_root: Some("Hematite".into()),
            explorer_entries: vec![
                WorkspaceEntry::dir("src", "src"),
                WorkspaceEntry::dir("src-tauri", "src-tauri"),
                WorkspaceEntry::dir("docs", "docs"),
                WorkspaceEntry::file("README.md", "README.md"),
                WorkspaceEntry::file("AGENTS.md", "AGENTS.md"),
            ],
            open_documents: Vec::new(),
            active_document: None,
            utility_tab: UtilityTab::default(),
            status_message: "Sample workspace ready.".into(),
        }
    }

    pub fn workspace_title(&self) -> &str {
        self.workspace_root.as_deref().unwrap_or("No workspace")
    }

    pub fn explorer_entries(&self) -> &[WorkspaceEntry] {
        &self.explorer_entries
    }

    pub fn explorer_labels(&self) -> Vec<String> {
        self.explorer_entries
            .iter()
            .map(|entry| {
                let prefix = if entry.is_dir { "[dir]" } else { "[file]" };
                format!("{prefix} {}", entry.name)
            })
            .collect()
    }

    pub fn status_message(&self) -> &str {
        &self.status_message
    }

    pub fn utility_tab(&self) -> UtilityTab {
        self.utility_tab
    }

    pub fn open_documents(&self) -> &[OpenDocument] {
        &self.open_documents
    }

    pub fn active_document(&self) -> Option<&OpenDocument> {
        self.active_document
            .and_then(|index| self.open_documents.get(index))
    }

    pub fn active_content(&self) -> &str {
        self.active_document()
            .map(|document| document.content.as_str())
            .unwrap_or("")
    }

    pub fn open_document(&mut self, path: impl Into<String>, content: impl Into<String>) -> usize {
        let path = path.into();
        if let Some(index) = self
            .open_documents
            .iter()
            .position(|document| document.path == path)
        {
            self.active_document = Some(index);
            self.status_message = format!("Focused {}", self.open_documents[index].title);
            return index;
        }

        let title = title_from_path(&path).to_owned();
        self.open_documents.push(OpenDocument {
            path,
            title: title.clone(),
            content: content.into(),
            dirty: false,
        });
        let index = self.open_documents.len() - 1;
        self.active_document = Some(index);
        self.status_message = format!("Opened {title}");
        index
    }

    pub fn edit_active_document(&mut self, content: impl Into<String>) -> Option<()> {
        let content = content.into();
        let index = self.active_document?;
        let document = self.open_documents.get_mut(index)?;
        if document.content != content {
            document.content = content;
            document.dirty = true;
            self.status_message = format!("Editing {}", document.title);
        }
        Some(())
    }

    pub fn save_active_document(&mut self) -> Option<SavedDocument> {
        let index = self.active_document?;
        let document = self.open_documents.get_mut(index)?;
        document.dirty = false;
        self.status_message = format!("Saved {}", document.title);
        Some(SavedDocument {
            path: document.path.clone(),
            content: document.content.clone(),
        })
    }

    pub fn select_utility_tab(&mut self, tab: UtilityTab) {
        self.utility_tab = tab;
        self.status_message = format!("Utility panel: {}", tab.label());
    }

    pub fn tab_labels(&self) -> Vec<String> {
        self.open_documents
            .iter()
            .map(|document| {
                if document.dirty {
                    format!("{} *", document.title)
                } else {
                    document.title.clone()
                }
            })
            .collect()
    }
}

fn title_from_path(path: &str) -> &str {
    path.rsplit(['/', '\\'])
        .next()
        .filter(|title| !title.is_empty())
        .unwrap_or("Untitled")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_starts_with_no_workspace_and_agents_tab() {
        let state = ShellState::default();

        assert_eq!(state.workspace_title(), "No workspace");
        assert_eq!(state.status_message(), "No workspace open.");
        assert_eq!(state.utility_tab(), UtilityTab::Agents);
        assert!(state.open_documents().is_empty());
    }

    #[test]
    fn opening_document_adds_active_tab() {
        let mut state = ShellState::with_sample_workspace();

        let index = state.open_document("src/main.rs", "fn main() {}\n");

        assert_eq!(index, 0);
        assert_eq!(state.active_document().unwrap().title, "main.rs");
        assert_eq!(state.active_document().unwrap().content, "fn main() {}\n");
        assert_eq!(state.tab_labels(), vec!["main.rs"]);
    }

    #[test]
    fn opening_existing_document_reuses_tab() {
        let mut state = ShellState::with_sample_workspace();

        state.open_document("src/main.rs", "first");
        let index = state.open_document("src/main.rs", "second");

        assert_eq!(index, 0);
        assert_eq!(state.open_documents().len(), 1);
        assert_eq!(state.active_document().unwrap().content, "first");
    }

    #[test]
    fn editing_active_document_marks_it_dirty() {
        let mut state = ShellState::with_sample_workspace();
        state.open_document("src/main.rs", "first");

        state.edit_active_document("changed");

        assert!(state.active_document().unwrap().dirty);
        assert_eq!(state.tab_labels(), vec!["main.rs *"]);
    }

    #[test]
    fn saving_active_document_clears_dirty_state() {
        let mut state = ShellState::with_sample_workspace();
        state.open_document("src/main.rs", "first");
        state.edit_active_document("changed");

        let saved = state.save_active_document().unwrap();

        assert_eq!(saved.path, "src/main.rs");
        assert_eq!(saved.content, "changed");
        assert!(!state.active_document().unwrap().dirty);
    }

    #[test]
    fn selecting_utility_tab_updates_state() {
        let mut state = ShellState::default();

        state.select_utility_tab(UtilityTab::Outline);

        assert_eq!(state.utility_tab(), UtilityTab::Outline);
        assert_eq!(state.status_message(), "Utility panel: Outline");
    }
}
