//! Pure conversion logic: ODK geometry strings, entity labels, and
//! property-name sanitization. Semantics ported from Pixel PR #185.

use std::collections::{HashMap, HashSet};
use geo::{Area, Geometry, InteriorPoint, Polygon, Simplify};

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

pub const DEFAULT_MAX_VERTICES: usize = 500;
const SIMPLIFY_START_TOLERANCE: f64 = 1e-5;
const SIMPLIFY_MAX_ROUNDS: usize = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Point,
    Polygon,
}

/// Decide the dataset's geometry family. Missing geometries are ignored
/// (they're skipped at conversion time); any non-point/polygon type, or a
/// mix of the two families, is an error naming the offending types and up
/// to 5 example row indices (1-based).
pub fn detect_family(geoms: &[Option<Geometry<f64>>]) -> Result<Family, String> {
    let mut families: HashSet<&'static str> = HashSet::new();
    let mut offenders: Vec<(usize, &'static str)> = Vec::new();
    for (i, g) in geoms.iter().enumerate() {
        let Some(g) = g else { continue };
        let name = geometry_type_name(g);
        match g {
            Geometry::Point(_) | Geometry::MultiPoint(_) => {
                families.insert("point");
            }
            Geometry::Polygon(_) | Geometry::MultiPolygon(_) => {
                families.insert("polygon");
            }
            _ => {
                if offenders.len() < 5 {
                    offenders.push((i + 1, name));
                }
                families.insert(name);
            }
        }
    }
    if !offenders.is_empty() {
        let examples: Vec<String> = offenders
            .iter()
            .map(|(row, t)| format!("row {row}: {t}"))
            .collect();
        return Err(format!(
            "unsupported geometry type(s) — only Point/MultiPoint and Polygon/MultiPolygon are supported ({})",
            examples.join(", ")
        ));
    }
    match (families.contains("point"), families.contains("polygon")) {
        (true, false) => Ok(Family::Point),
        (false, true) => Ok(Family::Polygon),
        (true, true) => Err(
            "mixed geometry families: dataset contains both Point and Polygon features".to_string(),
        ),
        (false, false) => Err("no geometries found in input".to_string()),
    }
}

fn geometry_type_name(g: &Geometry<f64>) -> &'static str {
    match g {
        Geometry::Point(_) => "Point",
        Geometry::MultiPoint(_) => "MultiPoint",
        Geometry::Polygon(_) => "Polygon",
        Geometry::MultiPolygon(_) => "MultiPolygon",
        Geometry::Line(_) | Geometry::LineString(_) => "LineString",
        Geometry::MultiLineString(_) => "MultiLineString",
        Geometry::GeometryCollection(_) => "GeometryCollection",
        Geometry::Rect(_) => "Rect",
        Geometry::Triangle(_) => "Triangle",
    }
}

/// ODK geopoint "lat lng 0 0" via interior point (always inside polygons;
/// MultiPoint yields a member point). None for empty geometries.
pub fn geopoint(geom: &Geometry<f64>) -> Option<String> {
    let p = geom.interior_point()?;
    Some(format!("{} {} 0 0", p.y(), p.x()))
}

fn largest_part(geom: &Geometry<f64>) -> Option<Polygon<f64>> {
    match geom {
        Geometry::Polygon(p) => Some(p.clone()),
        Geometry::MultiPolygon(mp) => mp
            .0
            .iter()
            .max_by(|a, b| {
                a.unsigned_area()
                    .partial_cmp(&b.unsigned_area())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .cloned(),
        _ => None,
    }
}

/// ODK geoshape: exterior ring of the largest part, holes dropped, closed,
/// Douglas-Peucker-simplified with growing tolerance until under
/// `max_vertices` (excluding the closing duplicate). None for non-polygonal
/// or empty geometries.
pub fn geoshape(geom: &Geometry<f64>, max_vertices: usize) -> Option<String> {
    let mut poly = largest_part(geom)?;
    if poly.exterior().0.is_empty() {
        return None;
    }
    let mut tolerance = SIMPLIFY_START_TOLERANCE;
    for _ in 0..SIMPLIFY_MAX_ROUNDS {
        if poly.exterior().0.len() <= max_vertices + 1 {
            break;
        }
        let simplified = poly.simplify(&tolerance);
        if !simplified.exterior().0.is_empty() {
            poly = simplified;
        }
        tolerance *= 4.0;
    }
    let ring = &poly.exterior().0;
    let parts: Vec<String> = ring.iter().map(|c| format!("{} {} 0 0", c.y, c.x)).collect();
    Some(parts.join(";"))
}
