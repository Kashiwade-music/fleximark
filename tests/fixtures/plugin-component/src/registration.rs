//! Role: Bridge typed fixture hooks to official WIT component ABI exports.
use fleximark_plugin_sdk::component::exports::fleximark::plugin::hooks::{
    Guest, Invocation, Response,
};

struct FixturePlugin;
impl Guest for FixturePlugin {
    fn invoke(request: Invocation) -> Result<Response, String> {
        fleximark_plugin_sdk::component::invoke(request, crate::hooks::invoke)
    }
}

// This reexport is emitted by the SDK's official wit_bindgen::generate! macro.
// Only generated component ABI exports are delegated to it.
fleximark_plugin_sdk::component::export!(FixturePlugin with_types_in fleximark_plugin_sdk::component);
