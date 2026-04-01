use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug, Clone, Default)]
pub struct ExtensionRegistry {
    extensions: Arc<RwLock<HashMap<Uuid, Extension>>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Extension {
    pub id: Uuid,
    pub name: String,
    pub publisher: String,
    pub version: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InstallExtensionParams {
    pub name: String,
    pub publisher: String,
    pub version: String,
}

impl ExtensionRegistry {
    pub async fn install(&self, params: InstallExtensionParams) -> Extension {
        let ext = Extension {
            id: Uuid::new_v4(),
            name: params.name,
            publisher: params.publisher,
            version: params.version,
            enabled: true,
        };

        self.extensions.write().await.insert(ext.id, ext.clone());
        ext
    }

    pub async fn list(&self) -> Vec<Extension> {
        self.extensions.read().await.values().cloned().collect()
    }
}
