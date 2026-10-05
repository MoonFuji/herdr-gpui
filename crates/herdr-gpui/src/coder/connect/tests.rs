#![allow(clippy::unwrap_used)]
use super::*;
use crate::coder::tests::settings;

#[test]
fn coder_ssh_carries_the_token_only_in_the_child_environment() {
    let command = ssh_command(
        Path::new("/usr/local/bin/coder"),
        &settings(),
        &"token-fixture".into(),
        "herdr-box",
        "main",
    )
    .unwrap();
    let args: Vec<_> = command.get_args().map(|a| a.to_str().unwrap()).collect();
    assert_eq!(args, ["ssh", "--wait=no", "--", "herdr-box.main"]);
    assert!(!args.iter().any(|arg| arg.contains("token-fixture")));
    let envs: std::collections::HashMap<_, _> = command
        .get_envs()
        .map(|(k, v)| (k.to_str().unwrap(), v.unwrap().to_str().unwrap()))
        .collect();
    assert_eq!(envs["CODER_SESSION_TOKEN"], "token-fixture");
    assert_eq!(envs["CODER_URL"], "https://coder.example.com");
    for (workspace, agent) in [("-oops", "main"), ("herdr-box", "a.b"), ("herdr-box", "")] {
        assert!(
            ssh_command(
                Path::new("coder"),
                &settings(),
                &"t".into(),
                workspace,
                agent
            )
            .is_err()
        );
    }
}

#[test]
fn a_configured_cli_path_wins_over_discovery() {
    let mut settings = settings();
    settings.cli = Some("/custom/coder".into());
    assert_eq!(cli(&settings).unwrap(), PathBuf::from("/custom/coder"));
}
