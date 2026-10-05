#![allow(clippy::unwrap_used)]
use super::*;

fn config() -> CoderConfig {
    CoderConfig {
        url: Some("https://coder.example.com/".into()),
        oauth_client_id: Some("0b0c1b3e-8f1a-4d6e-9d4a-8f0f6f0c2a11".into()),
        oauth_client_secret: Some("fixture-secret".into()),
        oauth_redirect_uri: Some("http://127.0.0.1:47823/callback".into()),
        ..CoderConfig::default()
    }
}

fn resolve(config: &CoderConfig) -> Result<Option<Settings>> {
    Settings::resolve(config, |_| None)
}

#[test]
fn unconfigured_deployment_is_absent_not_an_error() {
    assert!(resolve(&CoderConfig::default()).unwrap().is_none());
}

#[test]
fn valid_table_is_normalized_and_redirect_text_is_kept_verbatim() {
    let settings = resolve(&config()).unwrap().unwrap();
    assert_eq!(settings.base, "https://coder.example.com");
    assert_eq!(
        settings.endpoint("/oauth2/tokens"),
        "https://coder.example.com/oauth2/tokens"
    );
    assert_eq!(settings.redirect.uri, "http://127.0.0.1:47823/callback");
    assert_eq!(settings.redirect.path, "/callback");
    assert_eq!(
        settings.redirect.address,
        "127.0.0.1:47823".parse().unwrap()
    );
    assert_eq!(settings.workspace_prefix, "herdr");
    let debug = format!("{settings:?}");
    assert!(!debug.contains("fixture-secret"));

    let mut localhost = config();
    localhost.oauth_redirect_uri = Some("http://localhost:9000".into());
    let settings = resolve(&localhost).unwrap().unwrap();
    // Url would normalize this to `.../`; Coder must see what was registered.
    assert_eq!(settings.redirect.uri, "http://localhost:9000");
    assert_eq!(settings.redirect.address, "127.0.0.1:9000".parse().unwrap());
}

#[test]
fn environment_replaces_each_key() {
    let settings = Settings::resolve(&config(), |name| match name {
        "HERDR_CODER_URL" => Some("https://other.example.com/coder".into()),
        "HERDR_CODER_OAUTH_CLIENT_SECRET" => Some("env-secret".into()),
        _ => None,
    })
    .unwrap()
    .unwrap();
    assert_eq!(settings.base, "https://other.example.com/coder");
    assert_eq!(settings.client_secret.expose_secret(), "env-secret");
    let only_env = Settings::resolve(&CoderConfig::default(), |name| match name {
        "HERDR_CODER_URL" => Some("https://coder.example.com".into()),
        _ => None,
    });
    assert!(matches!(only_env, Err(Error::Missing("oauth_client_id"))));
}

#[test]
fn unsafe_or_ambiguous_values_are_rejected() {
    for (url, redirect) in [
        ("http://coder.example.com", "http://127.0.0.1:1/cb"),
        ("https://user:pw@coder.example.com", "http://127.0.0.1:1/cb"),
        ("https://coder.example.com?x=1", "http://127.0.0.1:1/cb"),
        ("https://coder.example.com#x", "http://127.0.0.1:1/cb"),
        ("ftp://coder.example.com", "http://127.0.0.1:1/cb"),
        ("not a url", "http://127.0.0.1:1/cb"),
        ("https://coder.example.com", "https://127.0.0.1:1/cb"),
        ("https://coder.example.com", "http://example.com:1/cb"),
        ("https://coder.example.com", "http://127.0.0.1/cb"),
        ("https://coder.example.com", "http://127.0.0.1:0/cb"),
        ("https://coder.example.com", "http://127.0.0.1:1/cb?x=1"),
    ] {
        let mut config = config();
        config.url = Some(url.into());
        config.oauth_redirect_uri = Some(redirect.into());
        assert!(resolve(&config).is_err(), "{url} {redirect}");
    }
    let mut local = config();
    local.url = Some("http://127.0.0.1:3000".into());
    assert!(resolve(&local).is_ok());
    for mutate in [
        (|c: &mut CoderConfig| c.oauth_client_id = Some("bad/id".into())) as fn(&mut _),
        |c| c.oauth_client_id = None,
        |c| c.oauth_client_secret = Some("bad secret".into()),
        |c| c.oauth_client_secret = None,
        |c| c.oauth_redirect_uri = None,
        |c| c.organization = Some("bad org".into()),
        |c| c.workspace_prefix = Some("Bad_Prefix".into()),
        |c| c.workspace_prefix = Some("a".repeat(PREFIX_LIMIT + 1)),
        |c| c.cli = Some("relative/coder".into()),
    ] {
        let mut config = config();
        mutate(&mut config);
        let error = resolve(&config).unwrap_err();
        assert!(!error.to_string().contains("fixture-secret"));
    }
}
