//! Pure conversion logic: ODK geometry strings, entity labels, and
//! property-name sanitization. Semantics ported from Pixel PR #185.

use std::collections::{HashMap, HashSet};

/// Per-row entity labels: the label column's trimmed value, else
/// "feature-<n>" (1-based). Duplicates get " (2)", " (3)"… in row order.
/// Generated suffixes are registered, so a literal "X (2)" value cannot
/// silently collide with a generated one. Deterministic: same input →
/// same labels (labels feed idempotency UUIDs downstream).
pub fn make_labels(values: Option<&[Option<String>]>, n_rows: usize) -> Vec<String> {
    let raw: Vec<String> = (0..n_rows)
        .map(|i| {
            let v = values.and_then(|vs| vs.get(i)).and_then(|v| v.as_deref());
            match v.map(str::trim) {
                Some(t) if !t.is_empty() => t.to_string(),
                _ => format!("feature-{}", i + 1),
            }
        })
        .collect();

    let mut seen: HashSet<String> = HashSet::new();
    let mut next_n: HashMap<String, usize> = HashMap::new();
    let mut out = Vec::with_capacity(raw.len());
    for base in raw {
        if seen.insert(base.clone()) {
            out.push(base);
            continue;
        }
        let mut n = next_n.get(&base).copied().unwrap_or(2);
        let label = loop {
            let candidate = format!("{base} ({n})");
            n += 1;
            if seen.insert(candidate.clone()) {
                break candidate;
            }
        };
        next_n.insert(base, n);
        out.push(label);
    }
    out
}

/// ODK-safe property names, same order as input. Rules: invalid chars → _;
/// digit-start prefixed _; leading __ collapsed to _; reserved
/// label/name/geometry get _ appended; collisions get _2, _3… suffixes.
pub fn sanitize_columns(cols: &[String]) -> Vec<String> {
    const RESERVED: [&str; 3] = ["label", "name", "geometry"];
    let mut taken: HashSet<String> = HashSet::new();
    taken.insert("label".to_string());
    taken.insert("geometry".to_string());
    let mut out = Vec::with_capacity(cols.len());
    for col in cols {
        let mut s: String = col
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-') {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        if s.is_empty() {
            s = "_".to_string();
        }
        if s.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            s = format!("_{s}");
        }
        while s.starts_with("__") {
            s.remove(0);
        }
        if RESERVED.contains(&s.as_str()) {
            s.push('_');
        }
        if !taken.insert(s.clone()) {
            let mut n = 2;
            s = loop {
                let candidate = format!("{s}_{n}");
                if taken.insert(candidate.clone()) {
                    break candidate;
                }
                n += 1;
            };
        }
        out.push(s);
    }
    out
}
