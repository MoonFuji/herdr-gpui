#![allow(clippy::unwrap_used)]
use super::*;

fn workspace(id: &str) -> SavedWorkspace {
    SavedWorkspace {
        id: id.into(),
        label: "Dev box".into(),
        deployment: "https://coder.example.com".into(),
        name: "herdr-dev-box".into(),
        session: "default".into(),
        enabled: true,
    }
}

#[test]
fn saves_replace_by_id_and_survive_reload() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join(FILE);
    assert!(read(&path).unwrap().is_empty());
    save_in(&path, workspace("w1")).unwrap();
    save_in(&path, workspace("w2")).unwrap();
    let mut renamed = workspace("w1");
    renamed.label = "Renamed".into();
    save_in(&path, renamed.clone()).unwrap();
    let saved = read(&path).unwrap();
    assert_eq!(saved.len(), 2);
    assert_eq!(saved[0], renamed);
    assert_eq!(saved[0].endpoint_id(), "coder:w1");
    assert_eq!(
        saved[0].target(),
        ConnectTarget::Coder {
            deployment: "https://coder.example.com".into(),
            workspace: "herdr-dev-box".into(),
            session: "default".into(),
        }
    );
}

#[test]
fn invalid_or_oversized_documents_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE);
    for bad in [
        SavedWorkspace {
            id: "../x".into(),
            ..workspace("w")
        },
        SavedWorkspace {
            name: "Bad Name".into(),
            ..workspace("w")
        },
        SavedWorkspace {
            session: "a/b".into(),
            ..workspace("w")
        },
        SavedWorkspace {
            label: "line\nbreak".into(),
            ..workspace("w")
        },
    ] {
        assert!(write(&path, vec![bad]).is_err());
    }
    for text in [
        r#"{"version":2,"workspaces":[]}"#,
        r#"{"version":1,"workspaces":[],"extra":1}"#,
        "not json",
    ] {
        fs::write(&path, text).unwrap();
        assert!(read(&path).is_err(), "{text}");
    }
    fs::write(&path, vec![b' '; LIMIT as usize + 1]).unwrap();
    assert!(read(&path).is_err());
    let many = (0..=MAX_WORKSPACES)
        .map(|i| workspace(&format!("w{i}")))
        .collect();
    assert!(write(&path, many).is_err());
}
