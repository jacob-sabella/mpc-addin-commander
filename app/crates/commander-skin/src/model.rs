//! The raw `TUI.json` document, as serde reads it. Only the fields the renderer uses are
//! named; everything else is ignored.

use serde::Deserialize;

/// The whole file.
#[derive(Debug, Clone, Deserialize)]
pub struct Tui {
    #[serde(rename = "pageData")]
    pub page_data: PageData,
}

/// The pages and the component definitions they are built from.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PageData {
    #[serde(default)]
    pub tabs: Vec<Tab>,
    #[serde(default)]
    pub component_definitions: ComponentDefinitions,
}

/// One page: an F-key tab, or a nested page under one.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Tab {
    #[serde(default)]
    pub tab_name: String,
    #[serde(default)]
    pub fn_key_index: u32,
    #[serde(default)]
    pub fn_key_sub_index: u32,
    /// The key of the page's component definition.
    #[serde(default)]
    pub component_name: String,
    /// `"x y w h"` of the page area in device pixels.
    #[serde(default)]
    pub initial_size: String,
}

/// The local definitions; `importFiles` (the device's own overlays) is ignored.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ComponentDefinitions {
    #[serde(default)]
    pub local_component_definitions: Vec<KeyValue>,
}

/// A named component definition.
#[derive(Debug, Clone, Deserialize)]
pub struct KeyValue {
    pub key: String,
    pub value: Definition,
}

/// A component definition: what a touch does, and its children.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Definition {
    #[serde(default)]
    pub actions: Vec<Action>,
    #[serde(default)]
    pub components_data: Vec<Component>,
}

/// One action of a definition.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    #[serde(default)]
    pub on_action: String,
    #[serde(default)]
    pub handler: String,
    #[serde(default)]
    pub handle_name: String,
}

/// A child of a definition: a built-in, or a placed instance of another definition.
#[derive(Debug, Clone, Deserialize)]
pub struct Component {
    #[serde(rename = "componentData")]
    pub component_data: ComponentData,
    #[serde(rename = "handle remapping", default)]
    pub handle_remapping: HandleRemapping,
    #[serde(default)]
    pub bounds: Bounds,
}

/// The child's type and type-specific data.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ComponentData {
    #[serde(default)]
    pub name: String,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub data: serde_json::Value,
}

/// Which parameter each handle of a placed instance binds to.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct HandleRemapping {
    #[serde(default)]
    pub map: Vec<Mapping>,
}

/// `{"key": "Data", "value": "Parameter 7"}`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Mapping {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub value: String,
}

/// Where a child sits in its parent and when it is drawn.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Bounds {
    /// `"x y w h"` relative to the parent's origin.
    #[serde(default)]
    pub bounds: String,
    /// `Always` or `WhenFocussed`.
    #[serde(default)]
    pub when_visible: String,
    /// `IndexedEnabling/<i>/<N>/Parameter <p>` conditions.
    #[serde(default)]
    pub additional_invalidating_handles: Vec<String>,
}
