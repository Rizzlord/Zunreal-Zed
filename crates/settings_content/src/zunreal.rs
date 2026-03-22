use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings_macros::MergeFrom;

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize, JsonSchema, MergeFrom)]
pub struct ZunrealSettingsContent {
    /// The path to the Unreal Engine installation directory.
    /// This should be the root directory containing 'Engine'.
    pub engine_path: Option<String>,
}
