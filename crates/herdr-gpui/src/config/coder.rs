//! The `[coder]` table: a Coder deployment whose workspaces become devices.
use super::{Config, LOCAL_CONFIG, write_config};
use crate::{Error, Result};
use serde::Deserialize;
use std::{
    env,
    ffi::OsString,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

/// A self-hosted Coder deployment whose workspaces can be added as devices.
/// Coder's OAuth2 provider requires a confidential client. Its secret may be
/// set here or in `HERDR_CODER_OAUTH_CLIENT_SECRET`, but Settings saves it to
/// the credential store instead; it is redacted from debug output.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CoderConfig {
    pub url: Option<String>,
    pub oauth_client_id: Option<String>,
    pub oauth_client_secret: Option<secrecy::SecretString>,
    pub oauth_redirect_uri: Option<String>,
    pub organization: Option<String>,
    pub workspace_prefix: Option<String>,
    /// Absolute path to the `coder` CLI when it is not on PATH.
    pub cli: Option<PathBuf>,
    pub allow_plaintext_credentials: bool,
}

impl CoderConfig {
    /// The validated deployment settings, or `None` when Coder is not set up.
    /// Each `HERDR_CODER_*` variable replaces the matching key.
    pub(crate) fn settings(&self) -> Result<Option<crate::coder::Settings>> {
        self.settings_with(|name| env::var_os(name))
    }

    pub(crate) fn settings_with(
        &self,
        var: impl Fn(&str) -> Option<OsString>,
    ) -> Result<Option<crate::coder::Settings>> {
        Ok(crate::coder::Settings::resolve(self, var)?)
    }
}

/// The `[coder]` keys the Settings window edits. An empty value removes its
/// key; the client secret and the plaintext opt-in are never written here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CoderFields {
    pub(crate) url: String,
    pub(crate) oauth_client_id: String,
    pub(crate) oauth_redirect_uri: String,
    pub(crate) organization: String,
    pub(crate) workspace_prefix: String,
    pub(crate) cli: String,
}

impl CoderFields {
    pub(crate) fn from_config(config: &CoderConfig) -> Self {
        let text = |value: &Option<String>| value.clone().unwrap_or_default();
        Self {
            url: text(&config.url),
            oauth_client_id: text(&config.oauth_client_id),
            oauth_redirect_uri: text(&config.oauth_redirect_uri),
            organization: text(&config.organization),
            workspace_prefix: text(&config.workspace_prefix),
            cli: config
                .cli
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
        }
    }

    fn entries(&self) -> [(&'static str, &str); 6] {
        [
            ("url", &self.url),
            ("oauth_client_id", &self.oauth_client_id),
            ("oauth_redirect_uri", &self.oauth_redirect_uri),
            ("organization", &self.organization),
            ("workspace_prefix", &self.workspace_prefix),
            ("cli", &self.cli),
        ]
    }

    /// The same checks loading applies, so a save cannot leave a config that
    /// then fails to load. A missing secret is allowed: Settings stores it.
    fn validate(&self, existing: &CoderConfig) -> Result<()> {
        let value = |text: &str| {
            let text = text.trim();
            (!text.is_empty()).then(|| text.to_owned())
        };
        let config = CoderConfig {
            url: value(&self.url),
            oauth_client_id: value(&self.oauth_client_id),
            oauth_redirect_uri: value(&self.oauth_redirect_uri),
            organization: value(&self.organization),
            workspace_prefix: value(&self.workspace_prefix),
            cli: value(&self.cli).map(PathBuf::from),
            ..existing.clone()
        };
        config.settings_with(|_| None).map(drop)
    }
}

impl Config {
    /// Write the `[coder]` keys Settings edits to the local override file.
    pub(crate) fn save_coder(fields: &CoderFields, existing: &CoderConfig) -> Result<()> {
        fields.validate(existing)?;
        let (_lock, local) = Self::prepare_files(&Self::path()?)?;
        Self::save_coder_path(fields, &local)
    }

    fn save_coder_path(fields: &CoderFields, path: &Path) -> Result<()> {
        let result = (|| -> Result<()> {
            let text = match fs::read_to_string(path) {
                Ok(text) => text,
                Err(error) if error.kind() == ErrorKind::NotFound => LOCAL_CONFIG.into(),
                Err(error) => return Err(error.into()),
            };
            let mut document = text.parse::<toml_edit::DocumentMut>()?;
            let table = document
                .entry("coder")
                .or_insert(toml_edit::Item::Table(toml_edit::Table::new()))
                .as_table_like_mut()
                .ok_or(crate::herdr_settings::Error::Table("coder"))?;
            for (key, value) in fields.entries() {
                let value = value.trim();
                if value.is_empty() {
                    table.remove(key);
                    continue;
                }
                let mut value = toml_edit::Value::from(value);
                if let Some(previous) = table.get(key).and_then(toml_edit::Item::as_value) {
                    *value.decor_mut() = previous.decor().clone();
                }
                table.insert(key, toml_edit::Item::Value(value));
            }
            if table.is_empty() {
                document.remove("coder");
            }
            write_config(path, &document.to_string())
        })();
        result.map_err(|error: Error| error.at_path(path))
    }
}

#[cfg(test)]
mod tests;
