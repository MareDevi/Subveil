//! Explicit service settings shared by GUI and cloud workers.
use std::collections::BTreeMap;

/// Invalid or missing explicit service option, without exposing its value.
#[derive(Debug, thiserror::Error)]
pub enum ServiceSettingsError {
    /// The application has no nonempty value for this field.
    #[error("请在应用内配置 {0}")]
    Missing(String),
    /// The value exceeds limits or contains control characters.
    #[error("{0} 无效")]
    Invalid(String),
}

/// Credentials and protocol options. Debug always redacts values; never log serialization.
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct ServiceSettings(pub BTreeMap<String, String>);
impl std::fmt::Debug for ServiceSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ServiceSettings([REDACTED])")
    }
}
impl ServiceSettings {
    /// Read an explicitly configured value, without any environment fallback.
    pub fn get(&self, name: &str) -> Result<String, ServiceSettingsError> {
        let value = self
            .0
            .get(name)
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| ServiceSettingsError::Missing(name.into()))?;
        if value.len() > 4096 || value.contains(['\r', '\n', '\0']) {
            return Err(ServiceSettingsError::Invalid(name.into()));
        }
        Ok(value.clone())
    }
    /// Read a nonempty optional option.
    pub fn optional(&self, name: &str) -> Option<String> {
        self.get(name).ok()
    }
    /// Set or remove one value. Validation occurs before service use.
    pub fn set(&mut self, name: &str, value: String) {
        if value.is_empty() {
            self.0.remove(name);
        } else {
            self.0.insert(name.into(), value);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_are_explicit_and_debug_is_redacted() {
        let mut settings = ServiceSettings::default();
        assert!(settings.get("HOME").is_err());
        settings.set("KEY", "sensitive-value".into());
        assert!(!format!("{settings:?}").contains("sensitive-value"));
        settings.set("KEY", "bad\nvalue".into());
        assert!(settings.get("KEY").is_err());
    }
}
