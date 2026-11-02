use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings_macros::MergeFrom;

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize, JsonSchema, MergeFrom)]
pub struct ZunrealSettingsContent {
    /// The path to the Unreal Engine installation directory.
    /// This should be the root directory containing 'Engine'.
    pub engine_path: Option<String>,
}

#[cfg(test)]
mod tests {
    use crate::{ParseStatus, RootUserSettings, UserSettingsContent};

    #[test]
    fn unreal_settings_survive_parsing_and_round_trip() -> anyhow::Result<()> {
        let json = r#"{
            "zunreal": { "engine_path": "/Apps/Unreal Engine 5" },
            "modeline_lines": 8,
            "windows": {
                "zunreal": { "engine_path": "C:\\Program Files\\Epic Games\\UE_5.7" }
            }
        }"#;
        let (settings, status) = UserSettingsContent::parse_json(json);
        assert_eq!(status, ParseStatus::Success);
        let settings = settings.ok_or_else(|| anyhow::anyhow!("settings were not parsed"))?;
        assert_eq!(
            settings
                .content
                .zunreal
                .as_ref()
                .and_then(|settings| settings.engine_path.as_deref()),
            Some("/Apps/Unreal Engine 5")
        );
        assert_eq!(settings.content.modeline_lines, Some(8));
        assert_eq!(
            settings
                .platform_overrides
                .windows
                .as_ref()
                .and_then(|settings| settings.zunreal.as_ref())
                .and_then(|settings| settings.engine_path.as_deref()),
            Some(r"C:\Program Files\Epic Games\UE_5.7")
        );

        let serialized = serde_json::to_string(&settings)?;
        let (restored, status) = UserSettingsContent::parse_json(&serialized);
        assert_eq!(status, ParseStatus::Success);
        assert_eq!(restored, Some(settings));
        Ok(())
    }

    #[test]
    fn unreal_settings_remain_optional() {
        let (settings, status) = UserSettingsContent::parse_json("{}");
        assert_eq!(status, ParseStatus::Success);
        assert!(settings.is_some_and(|settings| settings.content.zunreal.is_none()));
    }
}
