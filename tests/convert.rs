use odk_locations::convert::{make_labels, sanitize_columns};

fn s(v: &str) -> Option<String> {
    Some(v.to_string())
}

#[test]
fn labels_from_column_with_fallback_and_dedupe() {
    let vals = vec![s("A"), s("A"), s(""), None, s("A")];
    assert_eq!(
        make_labels(Some(&vals[..]), 5),
        vec!["A", "A (2)", "feature-3", "feature-4", "A (3)"]
    );
}

#[test]
fn labels_without_column_are_positional() {
    assert_eq!(make_labels(None, 3), vec!["feature-1", "feature-2", "feature-3"]);
}

#[test]
fn labels_trim_whitespace() {
    let vals = vec![s("  Clinic  "), s("   ")];
    assert_eq!(make_labels(Some(&vals[..]), 2), vec!["Clinic", "feature-2"]);
}

#[test]
fn label_dedupe_suffix_cannot_collide_with_literal_value() {
    // PR-185 deferred bug #1: literal "A (2)" must not collide with a
    // generated suffix. Suffixes are registered, so the collision resolves
    // deterministically.
    let vals = vec![s("A"), s("A"), s("A (2)")];
    let labels = make_labels(Some(&vals[..]), 3);
    assert_eq!(labels[0], "A");
    assert_eq!(labels[1], "A (2)");
    assert_eq!(labels[2], "A (2) (2)");
    // all unique
    let set: std::collections::HashSet<_> = labels.iter().collect();
    assert_eq!(set.len(), 3);
}

#[test]
fn sanitize_passthrough_and_invalid_chars() {
    let cols: Vec<String> = ["pop_2024", "région name", "a.b-c"]
        .iter()
        .map(|c| c.to_string())
        .collect();
    assert_eq!(sanitize_columns(&cols), vec!["pop_2024", "r_gion_name", "a.b-c"]);
}

#[test]
fn sanitize_reserved_digit_start_and_collisions() {
    let cols: Vec<String> = ["label", "name", "geometry", "2024", "a b", "a_b", "__x"]
        .iter()
        .map(|c| c.to_string())
        .collect();
    assert_eq!(
        sanitize_columns(&cols),
        vec!["label_", "name_", "geometry_", "_2024", "a_b", "a_b_2", "_x"]
    );
}
