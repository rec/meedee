use middi::midi::select_port_index;

#[test]
fn names_support_substrings_shell_patterns_and_unicode() {
    let names = ["FLOW 8 MIDI", "Other", "Clavé", "[literal", "bracket]"];
    for (pattern, expected) in [
        ("FLOW", 0),
        ("FLOW [0-9] MIDI", 0),
        ("[FO]LOW ? MIDI", 0),
        ("FLOW [!9] MIDI", 0),
        ("Clav[é]", 2),
        ("Clav?", 2),
        ("[[]literal", 3),
        ("[literal", 3),
        ("bracket[]]", 4),
        ("O[a-z]*", 1),
    ] {
        assert_eq!(
            select_port_index(names, Some(pattern)).unwrap(),
            expected,
            "{pattern}"
        );
    }
}

#[test]
fn selection_rejects_empty_ambiguous_and_missing_patterns() {
    assert_eq!(select_port_index(["first", "second"], None).unwrap(), 0);
    for (names, pattern) in [
        (vec![], None),
        (vec!["one"], Some("")),
        (vec!["same", "same"], Some("same")),
        (vec!["first", "second"], Some("*")),
        (vec!["one"], Some("two")),
        (vec!["Other"], Some("O[z-a]her")),
    ] {
        assert!(select_port_index(names, pattern).is_err());
    }
}
