use super::*;

#[test]
fn names_follow_coder_rules() {
    for name in ["a", "herdr-1", "a1-b2-c3", &"a".repeat(LIMIT)] {
        assert!(valid(name), "{name}");
    }
    for name in [
        "",
        "-a",
        "a-",
        "a--b",
        "A",
        "a_b",
        "a.b",
        &"a".repeat(LIMIT + 1),
    ] {
        assert!(!valid(name), "{name}");
    }
}

#[test]
fn suggestions_are_always_valid() {
    assert_eq!(suggest("herdr", "My Dev Box!"), "herdr-my-dev-box");
    assert_eq!(suggest("herdr", "  "), "herdr");
    assert_eq!(suggest("herdr", "Café au lait"), "herdr-caf-au-lait");
    let long = suggest("herdr", &"word ".repeat(20));
    assert!(long.len() <= LIMIT);
    for label in [
        "x-".repeat(40),
        "é".repeat(40),
        "a b c".into(),
        "---".into(),
    ] {
        assert!(valid(&suggest("herdr", &label)), "{label}");
    }
}

#[test]
fn random_names_are_valid_and_vary() {
    let names: std::collections::HashSet<_> = (0..64).map(|_| random("herdr")).collect();
    assert!(
        names.len() > 32,
        "names should rarely repeat: {}",
        names.len()
    );
    for name in &names {
        assert!(valid(name), "{name}");
        assert!(name.starts_with("herdr-"));
    }
    // The longest allowed prefix still yields a valid name.
    for _ in 0..64 {
        assert!(valid(&random(&"p".repeat(16))));
    }
    for (adjective, noun) in ADJECTIVES.iter().zip(NOUNS) {
        assert!(valid(adjective) && valid(noun));
    }
}
