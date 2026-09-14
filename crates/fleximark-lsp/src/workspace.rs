use std::sync::Arc;

use fleximark_engine::RenderConfig;
use fleximark_plugin_host::PluginHost;

#[derive(Clone, Debug, PartialEq, Eq)]
struct WorkspaceRoot(String);

struct RootedConfig {
    root: WorkspaceRoot,
    host: Arc<PluginHost>,
    render: RenderConfig,
}

#[derive(Default)]
pub(super) struct WorkspaceAuthority {
    default_host: Option<Arc<PluginHost>>,
    default_render: Option<RenderConfig>,
    rooted: Vec<RootedConfig>,
}

impl WorkspaceAuthority {
    pub(super) fn configure_default(
        &mut self,
        host: Option<PluginHost>,
        render: Option<RenderConfig>,
    ) {
        self.default_host = host.map(Arc::new);
        self.default_render = render;
    }

    pub(super) fn replace_roots(&mut self, workspaces: Vec<(String, PluginHost, RenderConfig)>) {
        self.rooted = workspaces
            .into_iter()
            .map(|(uri, host, render)| RootedConfig {
                root: WorkspaceRoot(uri.trim_end_matches('/').to_owned()),
                host: Arc::new(host),
                render,
            })
            .collect();
        self.sort_roots();
    }

    pub(super) fn matching(
        &self,
        document_uri: &str,
    ) -> Option<(&str, &Arc<PluginHost>, &RenderConfig)> {
        self.rooted
            .iter()
            .find(|entry| {
                document_uri == entry.root.0
                    || document_uri
                        .strip_prefix(&entry.root.0)
                        .is_some_and(|suffix| suffix.starts_with('/'))
            })
            .map(|entry| (entry.root.0.as_str(), &entry.host, &entry.render))
    }

    pub(super) fn exact(&self, workspace_uri: &str) -> Option<(&Arc<PluginHost>, &RenderConfig)> {
        self.rooted
            .iter()
            .find(|entry| entry.root.0 == workspace_uri)
            .map(|entry| (&entry.host, &entry.render))
    }

    pub(super) fn default_host(&self) -> Option<&Arc<PluginHost>> {
        self.default_host.as_ref()
    }

    pub(super) fn default_render(&self) -> Option<&RenderConfig> {
        self.default_render.as_ref()
    }

    pub(super) fn replace(
        &mut self,
        workspace_uri: &str,
        host: Arc<PluginHost>,
        render: RenderConfig,
    ) {
        if let Some(entry) = self
            .rooted
            .iter_mut()
            .find(|entry| entry.root.0 == workspace_uri)
        {
            entry.host = host;
            entry.render = render;
        } else {
            self.rooted.push(RootedConfig {
                root: WorkspaceRoot(workspace_uri.to_owned()),
                host,
                render,
            });
            self.sort_roots();
        }
    }

    fn sort_roots(&mut self) {
        self.rooted
            .sort_by_key(|entry| std::cmp::Reverse(entry.root.0.len()));
    }
}
