use std::{collections::HashMap, sync::Arc};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Default)]
pub struct WorkspaceStore {
    files: Arc<RwLock<HashMap<String, String>>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OpenFileParams {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SaveFileParams {
    pub path: String,
    pub content: String,
}

impl WorkspaceStore {
    pub async fn open(&self, path: String, content: String) {
        self.files.write().await.insert(path, content);
    }

    pub async fn save(&self, path: String, content: String) {
        self.files.write().await.insert(path, content);
    }

    pub async fn get(&self, path: &str) -> Option<String> {
        self.files.read().await.get(path).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn open_and_read_file() {
        let store = WorkspaceStore::default();
        store.open("main.rs".into(), "fn main() {}".into()).await;

        assert_eq!(store.get("main.rs").await.as_deref(), Some("fn main() {}"));
    }

    #[tokio::test]
    async fn save_overwrites_content() {
        let store = WorkspaceStore::default();
        store.save("main.rs".into(), "old".into()).await;
        store.save("main.rs".into(), "new".into()).await;

        assert_eq!(store.get("main.rs").await.as_deref(), Some("new"));
    }
}
