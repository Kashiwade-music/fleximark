//! Role: Apply workspace folder additions and detach removals without a restart.
use super::state::*;
#[derive(serde::Deserialize)]
struct Folder {
    uri: String,
    #[serde(rename = "name")]
    _name: Option<String>,
}
#[derive(serde::Deserialize)]
struct FolderChanges {
    added: Vec<Folder>,
    removed: Vec<Folder>,
}
#[derive(serde::Deserialize)]
struct Params {
    event: FolderChanges,
}
impl Server {
    pub(super) fn change_workspace_folders(
        &mut self,
        id: Option<Value>,
        params: Value,
    ) -> Option<Value> {
        if let Some(id) = id {
            return Some(super::wire::invalid_params(
                id,
                "workspace folder changes must be a notification",
            ));
        }
        let result = (|| {
            let params: Params = serde_json::from_value(params)?;
            let mut affected = std::collections::HashSet::new();
            for folder in params.event.removed {
                let uri = folder.uri.trim_end_matches('/').to_owned();
                affected.extend(
                    self.registry
                        .workspace_documents(&uri)
                        .into_iter()
                        .map(|(uri, _)| uri),
                );
                self.registry.remove_workspace(&uri)?;
            }
            for folder in params.event.added {
                let uri = folder.uri.trim_end_matches('/').to_owned();
                self.registry.add_workspace(uri.clone())?;
                affected.extend(
                    self.registry
                        .workspace_documents(&uri)
                        .into_iter()
                        .map(|(uri, _)| uri),
                );
            }
            for uri in affected {
                self.publish_document_previews(&uri)?;
            }
            Ok::<_, ApplicationError>(())
        })();
        if let Err(error) = result {
            crate::telemetry::log_operational_failure(
                "workspace-folders-failed",
                &error,
                None,
                None,
                None,
            );
        }
        None
    }
}
