//! TOML configuration shared by the control window and overlay.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Which subtitle text to display.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayMode {
    /// Show source and translation.
    #[default]
    Bilingual,
    /// Show recognized source text only.
    Source,
    /// Show translated text only.
    Translation,
}

/// Validated before any configuration is applied to a window.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct OverlaySettings {
    /// Preferred output UUID. None follows the first output in layout order.
    pub output_uuid: Option<String>,
    /// Font family; missing installed glyphs use the platform fallback.
    pub font_family: String,
    /// Translation font size in logical pixels.
    pub font_size: f32,
    /// Source font size relative to the translation.
    pub source_ratio: f32,
    /// Background opacity, independent of text opacity.
    pub background_opacity: f32,
    /// Source RGB color.
    pub source_color: u32,
    /// Translation RGB color.
    pub translation_color: u32,
    /// Panel RGB color.
    pub background_color: u32,
    /// Requested width in logical pixels, constrained to the display at runtime.
    pub width: f32,
    /// Distance from the bottom of the display in logical pixels.
    pub bottom_margin: f32,
    /// Maximum wrapped lines per language.
    pub max_lines: usize,
    /// Whether pointer and touch input pass through the surface.
    pub click_through: bool,
    /// Language layout.
    pub display_mode: DisplayMode,
    /// Group adjacent short completed pairs into one reading slot.
    pub merge_short_segments: bool,
    /// Inactivity time before a subtitle expires.
    pub ttl_ms: u64,
    /// Maximum retained subtitle segments.
    pub history_limit: usize,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            output_uuid: None,
            font_family: "sans-serif".into(),
            font_size: 32.,
            source_ratio: 0.75,
            background_opacity: 0.65,
            source_color: 0xcbd5e1,
            translation_color: 0xffffff,
            background_color: 0x10151e,
            width: 960.,
            bottom_margin: 64.,
            max_lines: 3,
            click_through: true,
            display_mode: DisplayMode::Bilingual,
            merge_short_segments: false,
            ttl_ms: 8_000,
            history_limit: 32,
        }
    }
}

/// A malformed or out-of-range configuration.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// TOML decoding failed.
    #[error("invalid subtitle configuration: {0}")]
    Decode(#[from] toml::de::Error),
    /// TOML encoding failed.
    #[error("cannot encode subtitle configuration: {0}")]
    Encode(#[from] toml::ser::Error),
    /// A named field is outside its supported range.
    #[error("invalid subtitle setting: {0}")]
    Invalid(&'static str),
}

impl OverlaySettings {
    /// Decode and validate settings, filling omitted fields from defaults.
    pub fn from_toml(text: &str) -> Result<Self, ConfigError> {
        let settings: Self = toml::from_str(text)?;
        settings.validate()?;
        Ok(settings)
    }

    /// Encode validated settings for explicit persistence.
    pub fn to_toml(&self) -> Result<String, ConfigError> {
        self.validate()?;
        Ok(toml::to_string_pretty(self)?)
    }

    /// Reject non-finite values and excessive geometry or retention limits.
    pub fn validate(&self) -> Result<(), ConfigError> {
        for (name, value, min, max) in [
            ("font_size (18..64)", self.font_size, 18., 64.),
            ("source_ratio (0.5..1)", self.source_ratio, 0.5, 1.),
            ("background_opacity (0..1)", self.background_opacity, 0., 1.),
            ("width (320..2400)", self.width, 320., 2400.),
            ("bottom_margin (0..400)", self.bottom_margin, 0., 400.),
        ] {
            if !value.is_finite() || !(min..=max).contains(&value) {
                return Err(ConfigError::Invalid(name));
            }
        }
        if self.output_uuid.as_ref().is_some_and(|id| {
            id.len() != 36
                || !id.bytes().enumerate().all(|(i, b)| {
                    if matches!(i, 8 | 13 | 18 | 23) {
                        b == b'-'
                    } else {
                        b.is_ascii_hexdigit()
                    }
                })
        }) {
            return Err(ConfigError::Invalid("output_uuid"));
        }
        if self.font_family.trim().is_empty() || self.font_family.len() > 128 {
            return Err(ConfigError::Invalid("font_family (1..128 bytes)"));
        }
        if !(1..=6).contains(&self.max_lines) {
            return Err(ConfigError::Invalid("max_lines (1..6)"));
        }
        if !(1_000..=60_000).contains(&self.ttl_ms) {
            return Err(ConfigError::Invalid("ttl_ms (1000..60000)"));
        }
        if !(1..=256).contains(&self.history_limit) {
            return Err(ConfigError::Invalid("history_limit (1..256)"));
        }
        if [
            self.source_color,
            self.translation_color,
            self.background_color,
        ]
        .iter()
        .any(|color| *color > 0xffffff)
        {
            return Err(ConfigError::Invalid("RGB color (0x000000..0xffffff)"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_preserves_custom_values() -> Result<(), ConfigError> {
        let settings = OverlaySettings {
            output_uuid: Some("12345678-1234-1234-1234-123456789abc".into()),
            font_family: "Noto Sans CJK SC".into(),
            background_opacity: 0.,
            display_mode: DisplayMode::Translation,
            ..Default::default()
        };
        assert_eq!(OverlaySettings::from_toml(&settings.to_toml()?)?, settings);
        Ok(())
    }

    #[test]
    fn partial_config_uses_defaults() -> Result<(), ConfigError> {
        assert_eq!(OverlaySettings::from_toml("font_size = 40")?.width, 960.);
        Ok(())
    }

    #[test]
    fn invalid_config_rejects_nan_ranges_unknown_fields_and_bad_colors() {
        for text in [
            "font_size = nan",
            "background_opacity = inf",
            "width = 0",
            "font_family = ''",
            "max_lines = 0",
            "ttl_ms = 0",
            "history_limit = 999999",
            "translation_color = 16777216",
            "widht = 900",
            "output_uuid = 'wrong'",
        ] {
            assert!(OverlaySettings::from_toml(text).is_err(), "accepted {text}");
        }
    }
}
