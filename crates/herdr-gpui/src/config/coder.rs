//! The `[coder]` table: a Coder deployment whose workspaces become devices.
use crate::Result;
use serde::Deserialize;
use std::{env, ffi::OsString, path::PathBuf};

/// A self-hosted Coder deployment whose workspaces can be added as devices.
/// Coder's OAuth2 provider requires a confidential client, so the secret is
/// configured here or in `HERDR_CODER_OAUTH_CLIENT_SECRET`; it is redacted from
/// debug output and never written back by the GUI.
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
