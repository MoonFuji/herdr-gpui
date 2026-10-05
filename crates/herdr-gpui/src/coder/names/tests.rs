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
