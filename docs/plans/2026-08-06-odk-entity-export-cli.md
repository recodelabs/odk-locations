# ODK Entity-CSV Converter CLI (Rust) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A single static Rust binary `odk-locations` converting GeoJSON / GeoParquet / plain-parquet / CSV point-or-polygon data into an ODK entity CSV (`label`, `geometry` in ODK geopoint/geoshape format, all attributes passed through).

**Architecture:** bin+lib crate: `input.rs` (per-format readers → `Dataset`), `convert.rs` (pure geometry/label/sanitize logic), `output.rs` (CSV writer + summary), `main.rs` (clap wiring). Semantics ported from Pixel PR #185 via test vectors. Spec: `docs/specs/2026-08-06-odk-entity-export-cli-design.md`.

**Tech Stack:** Rust 1.97 (pinned in `.tool-versions`), clap 4 (derive), geo 0.30, geojson 0.24, geozero 0.14, parquet/arrow 56, proj4rs 0.1 + crs-definitions 0.3, csv 1, serde_json 1 (preserve_order), anyhow 1; dev: assert_cmd 2, predicates 3.

## Global Constraints

- ODK geometry formats: geopoint `"{lat} {lng} 0 0"`; geoshape = same tuples joined by `;` (no space), ring closed (first vertex repeated last). Coordinates use Rust's default `f64` `Display` (shortest round-trip).
- Geoshape vertex cap default 500 (excluding the closing duplicate); simplify tolerance starts 1e-5, ×4 per round, max 30 rounds.
- Labels are deterministic; dedupe registers generated suffixes (literal `"X (2)"` cannot collide); null detection covers all null-ish values.
- Sanitized property names: allowed chars `[A-Za-z0-9._-]`, others → `_`; digit-start prefixed `_`; leading `__` collapsed to `_`; reserved `label`/`name`/`geometry` get `_` appended; post-sanitize collisions get `_2`, `_3`…
- Pure Rust only — no libproj/GDAL. Unsupported CRS is a fatal error, not a silent pass-through.
- Summary/warnings → stderr; exit code 1 on fatal errors.
- **Crate APIs were compile-verified 2026-08-06 on the exact pinned versions** (geo 0.30.0, geojson 0.24.2, geozero 0.14.0, proj4rs 0.1.10, crs-definitions 0.3.1, parquet/arrow 56.2.1). If a signature still differs when you build, adapt minimally — the tests define the semantics.
- Rust runs through asdf: `.tool-versions` (already committed intent: `rust 1.97.1`) must exist at repo root or cargo won't resolve.
- Work directly on `main` of `~/github/odk-locations` (fresh repo, no remote yet). Commit per task step as usual.

---

### Task 1: Scaffold + labels & sanitization (`convert.rs` part 1)

**Files:**
- Create: `Cargo.toml`, `.gitignore`, `src/main.rs` (stub), `src/lib.rs`, `src/convert.rs`
- Test: `tests/convert.rs`

**Interfaces:**
- Produces (used by every later task):
  - `odk_locations::convert::make_labels(values: Option<&[Option<String>]>, n_rows: usize) -> Vec<String>`
  - `odk_locations::convert::sanitize_columns(cols: &[String]) -> Vec<String>` (same length/order as input)

- [ ] **Step 1: Scaffold the crate**

`Cargo.toml`:

```toml
[package]
name = "odk-locations"
version = "0.1.0"
edition = "2021"

[lib]
name = "odk_locations"
path = "src/lib.rs"

[[bin]]
name = "odk-locations"
path = "src/main.rs"

[dependencies]
anyhow = "1"
clap = { version = "4", features = ["derive"] }
csv = "1"
geo = "0.30"
geojson = "0.24"
geozero = { version = "0.14", features = ["with-wkb", "with-geo"] }
parquet = "56"
arrow = "56"
proj4rs = "0.1"
crs-definitions = "0.3"
serde_json = { version = "1", features = ["preserve_order"] }

[dev-dependencies]
assert_cmd = "2"
predicates = "3"

[profile.release]
strip = true
lto = true
```

`.gitignore`:

```
/target
```

`src/lib.rs`:

```rust
pub mod convert;
```

`src/main.rs` (stub for now — Task 5 replaces it):

```rust
fn main() {
    eprintln!("not implemented yet");
    std::process::exit(1);
}
```

`src/convert.rs` starts empty except `#![allow(dead_code)]`-free module doc:

```rust
//! Pure conversion logic: ODK geometry strings, entity labels, and
//! property-name sanitization. Semantics ported from Pixel PR #185.
```

Confirm `.tool-versions` exists at repo root containing `rust 1.97.1` (create it if missing).

- [ ] **Step 2: Write the failing tests**

`tests/convert.rs`:

```rust
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
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --test convert`
Expected: compile FAIL — `make_labels`/`sanitize_columns` not found.

- [ ] **Step 4: Implement**

Append to `src/convert.rs`:

```rust
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
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --test convert`
Expected: 6 passed.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock .gitignore .tool-versions src/lib.rs src/main.rs src/convert.rs tests/convert.rs
git commit -m "feat: crate scaffold + entity labels and property-name sanitization"
```

---

### Task 2: Geometry strings (`convert.rs` part 2)

**Files:**
- Modify: `src/convert.rs`
- Test: `tests/convert.rs` (append)

**Interfaces:**
- Consumes: nothing new.
- Produces (used by Task 5):
  - `odk_locations::convert::Family` — `enum Family { Point, Polygon }` (derive `Debug, Clone, Copy, PartialEq, Eq`)
  - `odk_locations::convert::detect_family(geoms: &[Option<geo::Geometry<f64>>]) -> Result<Family, String>`
  - `odk_locations::convert::geopoint(geom: &geo::Geometry<f64>) -> Option<String>`
  - `odk_locations::convert::geoshape(geom: &geo::Geometry<f64>, max_vertices: usize) -> Option<String>`
  - `odk_locations::convert::DEFAULT_MAX_VERTICES: usize = 500`

- [ ] **Step 1: Write the failing tests**

Append to `tests/convert.rs`:

```rust
use geo::{Geometry, LineString, MultiPoint, MultiPolygon, Point, Polygon};
use odk_locations::convert::{detect_family, geopoint, geoshape, Family, DEFAULT_MAX_VERTICES};

fn square(x0: f64, y0: f64, size: f64) -> Polygon<f64> {
    Polygon::new(
        LineString::from(vec![
            (x0, y0),
            (x0 + size, y0),
            (x0 + size, y0 + size),
            (x0, y0 + size),
            (x0, y0),
        ]),
        vec![],
    )
}

#[test]
fn geopoint_is_lat_lng_zero_zero() {
    let g = Geometry::Point(Point::new(3.4, 6.5));
    assert_eq!(geopoint(&g).unwrap(), "6.5 3.4 0 0");
}

#[test]
fn geopoint_multipoint_uses_member_point() {
    let g = Geometry::MultiPoint(MultiPoint::from(vec![(1.0, 1.0), (3.0, 1.0)]));
    let s = geopoint(&g).unwrap();
    let parts: Vec<&str> = s.split(' ').collect();
    assert_eq!(parts.len(), 4);
    let lat: f64 = parts[0].parse().unwrap();
    let lng: f64 = parts[1].parse().unwrap();
    assert_eq!(lat, 1.0);
    assert!((1.0..=3.0).contains(&lng));
}

#[test]
fn geopoint_polygon_is_inside() {
    let g = Geometry::Polygon(square(0.0, 0.0, 2.0));
    let s = geopoint(&g).unwrap();
    let parts: Vec<&str> = s.split(' ').collect();
    let lat: f64 = parts[0].parse().unwrap();
    let lng: f64 = parts[1].parse().unwrap();
    assert!(lat > 0.0 && lat < 2.0 && lng > 0.0 && lng < 2.0);
}

#[test]
fn geoshape_is_closed_exterior_ring_holes_dropped() {
    let poly = Polygon::new(
        LineString::from(vec![(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0), (0.0, 0.0)]),
        vec![LineString::from(vec![
            (1.0, 1.0),
            (2.0, 1.0),
            (2.0, 2.0),
            (1.0, 2.0),
            (1.0, 1.0),
        ])],
    );
    let s = geoshape(&Geometry::Polygon(poly), DEFAULT_MAX_VERTICES).unwrap();
    let parts: Vec<&str> = s.split(';').collect();
    assert_eq!(parts.first(), parts.last());
    assert_eq!(parts.len(), 5); // 4 corners + closing duplicate; hole gone
    assert!(parts.iter().all(|p| p.ends_with(" 0 0")));
}

#[test]
fn geoshape_multipolygon_takes_largest_part() {
    let mp = MultiPolygon::new(vec![square(0.0, 0.0, 1.0), square(10.0, 10.0, 5.0)]);
    let s = geoshape(&Geometry::MultiPolygon(mp), DEFAULT_MAX_VERTICES).unwrap();
    let first_lat: f64 = s.split(' ').next().unwrap().parse().unwrap();
    assert!(first_lat >= 10.0);
}

#[test]
fn geoshape_simplifies_under_vertex_cap() {
    let n = 2000;
    let ring: Vec<(f64, f64)> = (0..=n)
        .map(|i| {
            let t = 2.0 * std::f64::consts::PI * (i as f64) / (n as f64);
            (t.cos(), t.sin())
        })
        .collect();
    let poly = Polygon::new(LineString::from(ring), vec![]);
    let s = geoshape(&Geometry::Polygon(poly), DEFAULT_MAX_VERTICES).unwrap();
    let parts: Vec<&str> = s.split(';').collect();
    assert!(parts.len() <= DEFAULT_MAX_VERTICES + 1);
    assert_eq!(parts.first(), parts.last());
}

#[test]
fn detect_family_points_polygons_and_mixed() {
    let pts = vec![
        Some(Geometry::Point(Point::new(0.0, 0.0))),
        None,
        Some(Geometry::MultiPoint(MultiPoint::from(vec![(1.0, 1.0)]))),
    ];
    assert_eq!(detect_family(&pts).unwrap(), Family::Point);

    let polys = vec![Some(Geometry::Polygon(square(0.0, 0.0, 1.0)))];
    assert_eq!(detect_family(&polys).unwrap(), Family::Polygon);

    let mixed = vec![
        Some(Geometry::Point(Point::new(0.0, 0.0))),
        Some(Geometry::Polygon(square(0.0, 0.0, 1.0))),
    ];
    let err = detect_family(&mixed).unwrap_err();
    assert!(err.contains("Point") && err.contains("Polygon"));

    let line = vec![Some(Geometry::LineString(LineString::from(vec![
        (0.0, 0.0),
        (1.0, 1.0),
    ])))];
    let err = detect_family(&line).unwrap_err();
    assert!(err.contains("LineString"));
    assert!(err.contains("row"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test convert`
Expected: compile FAIL — new items not found.

- [ ] **Step 3: Implement**

Append to `src/convert.rs`:

```rust
use geo::{Area, Geometry, InteriorPoint, Polygon, Simplify};

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
```

Note: `use std::collections::{HashMap, HashSet};` already exists from Task 1 — merge, don't duplicate.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test convert`
Expected: 13 passed.

- [ ] **Step 5: Commit**

```bash
git add src/convert.rs tests/convert.rs
git commit -m "feat: ODK geopoint/geoshape strings and geometry-family detection"
```

---

### Task 3: GeoJSON + CSV readers (`input.rs`)

**Files:**
- Create: `src/input.rs`, `tests/fixtures/points.geojson`, `tests/fixtures/polys.geojson`, `tests/fixtures/points.csv`
- Modify: `src/lib.rs` (add `pub mod input;`)
- Test: `tests/input.rs`

**Interfaces:**
- Consumes: nothing from convert.
- Produces (used by Tasks 4-5):
  - `odk_locations::input::Dataset { pub geoms: Vec<Option<geo::Geometry<f64>>>, pub columns: Vec<String>, pub rows: Vec<Vec<Option<String>>> }` (rows aligned with columns; values stringified, null → None)
  - `odk_locations::input::read_input(path: &std::path::Path, lat_col: Option<&str>, lng_col: Option<&str>) -> anyhow::Result<Dataset>` — dispatches on extension: `.geojson`/`.json` → GeoJSON, `.csv` → CSV, `.parquet` → parquet (Task 4 fills that arm; until then it returns `Err(anyhow!("parquet support not implemented yet"))`).
  - `odk_locations::input::detect_latlng(columns: &[String], lat: Option<&str>, lng: Option<&str>) -> anyhow::Result<(usize, usize)>` — indices of lat/lng columns; explicit names override; candidates (case-insensitive): lat `["lat","latitude","y"]`, lng `["lng","lon","long","longitude","x"]`.

- [ ] **Step 1: Create the fixtures**

`tests/fixtures/points.geojson`:

```json
{
  "type": "FeatureCollection",
  "features": [
    {"type": "Feature", "geometry": {"type": "Point", "coordinates": [3.4, 6.5]},
     "properties": {"name": "Clinic A", "pop": 1200, "active": true}},
    {"type": "Feature", "geometry": {"type": "Point", "coordinates": [3.5, 6.6]},
     "properties": {"name": "Clinic B", "pop": 800, "active": false}},
    {"type": "Feature", "geometry": null,
     "properties": {"name": null, "pop": 5, "active": true}}
  ]
}
```

`tests/fixtures/polys.geojson`:

```json
{
  "type": "FeatureCollection",
  "features": [
    {"type": "Feature",
     "geometry": {"type": "Polygon", "coordinates": [[[0,0],[4,0],[4,4],[0,4],[0,0]],[[1,1],[2,1],[2,2],[1,2],[1,1]]]},
     "properties": {"name": "holey"}},
    {"type": "Feature",
     "geometry": {"type": "MultiPolygon", "coordinates": [[[[0,0],[1,0],[1,1],[0,1],[0,0]]],[[[10,10],[15,10],[15,15],[10,15],[10,10]]]]},
     "properties": {"name": "mp"}}
  ]
}
```

`tests/fixtures/points.csv`:

```csv
name,Latitude,LON,notes
Site 1,6.5,3.4,ok
Site 2,6.6,3.5,
Site 3,,,missing coords
```

- [ ] **Step 2: Write the failing tests**

`tests/input.rs`:

```rust
use odk_locations::input::{detect_latlng, read_input, Dataset};
use std::path::Path;

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

#[test]
fn geojson_points_columns_rows_and_nulls() {
    let ds: Dataset = read_input(&fixture("points.geojson"), None, None).unwrap();
    assert_eq!(ds.columns, vec!["name", "pop", "active"]);
    assert_eq!(ds.geoms.len(), 3);
    assert!(ds.geoms[0].is_some());
    assert!(ds.geoms[2].is_none());
    assert_eq!(ds.rows[0], vec![Some("Clinic A".into()), Some("1200".into()), Some("true".into())]);
    assert_eq!(ds.rows[2][0], None); // null property -> None
}

#[test]
fn geojson_polygons_parse() {
    let ds = read_input(&fixture("polys.geojson"), None, None).unwrap();
    assert_eq!(ds.geoms.len(), 2);
    assert!(matches!(ds.geoms[0], Some(geo::Geometry::Polygon(_))));
    assert!(matches!(ds.geoms[1], Some(geo::Geometry::MultiPolygon(_))));
}

#[test]
fn csv_autodetects_latlng_case_insensitive() {
    let ds = read_input(&fixture("points.csv"), None, None).unwrap();
    // lat/lng columns are consumed for geometry, not exported as attributes
    assert_eq!(ds.columns, vec!["name", "notes"]);
    assert_eq!(ds.geoms.len(), 3);
    let p = match &ds.geoms[0] {
        Some(geo::Geometry::Point(p)) => p,
        other => panic!("expected point, got {other:?}"),
    };
    assert_eq!((p.y(), p.x()), (6.5, 3.4));
    assert!(ds.geoms[2].is_none()); // unparseable/empty coords -> None
    assert_eq!(ds.rows[1], vec![Some("Site 2".into()), None]); // empty csv cell -> None
}

#[test]
fn csv_explicit_latlng_flags_override() {
    let ds = read_input(&fixture("points.csv"), Some("Latitude"), Some("LON")).unwrap();
    assert_eq!(ds.columns, vec!["name", "notes"]);
    assert!(ds.geoms[0].is_some());
}

#[test]
fn detect_latlng_errors_when_missing() {
    let cols: Vec<String> = vec!["a".into(), "b".into()];
    let err = detect_latlng(&cols, None, None).unwrap_err().to_string();
    assert!(err.contains("lat"), "{err}");
    let err = detect_latlng(&cols, Some("nope"), Some("b")).unwrap_err().to_string();
    assert!(err.contains("nope"), "{err}");
}

#[test]
fn unknown_extension_fails() {
    let err = read_input(Path::new("data.shp"), None, None).unwrap_err().to_string();
    assert!(err.contains(".shp") || err.to_lowercase().contains("unsupported"), "{err}");
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --test input`
Expected: compile FAIL — module `input` not found.

- [ ] **Step 4: Implement**

Add `pub mod input;` to `src/lib.rs`. Create `src/input.rs`:

```rust
//! Per-format readers producing a uniform Dataset: geometries plus
//! stringified attribute columns in input order.

use anyhow::{anyhow, bail, Context, Result};
use geo::Geometry;
use std::path::Path;

pub struct Dataset {
    pub geoms: Vec<Option<Geometry<f64>>>,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
}

const LAT_CANDIDATES: [&str; 3] = ["lat", "latitude", "y"];
const LNG_CANDIDATES: [&str; 5] = ["lng", "lon", "long", "longitude", "x"];

/// Find the lat/lng column indices. Explicit names (from CLI flags) override
/// autodetection and error if absent; otherwise the first case-insensitive
/// candidate match wins.
pub fn detect_latlng(
    columns: &[String],
    lat: Option<&str>,
    lng: Option<&str>,
) -> Result<(usize, usize)> {
    let find_explicit = |name: &str| {
        columns
            .iter()
            .position(|c| c == name)
            .ok_or_else(|| anyhow!("column {name:?} not found in input (columns: {columns:?})"))
    };
    let find_candidate = |cands: &[&str], what: &str| {
        columns
            .iter()
            .position(|c| cands.contains(&c.to_ascii_lowercase().as_str()))
            .ok_or_else(|| {
                anyhow!(
                    "could not autodetect a {what} column (tried {cands:?}); \
                     pass --lat-column/--lng-column (columns: {columns:?})"
                )
            })
    };
    let lat_idx = match lat {
        Some(name) => find_explicit(name)?,
        None => find_candidate(&LAT_CANDIDATES, "lat")?,
    };
    let lng_idx = match lng {
        Some(name) => find_explicit(name)?,
        None => find_candidate(&LNG_CANDIDATES, "lng")?,
    };
    Ok((lat_idx, lng_idx))
}

pub fn read_input(path: &Path, lat_col: Option<&str>, lng_col: Option<&str>) -> Result<Dataset> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "geojson" | "json" => read_geojson(path),
        "csv" => read_csv(path, lat_col, lng_col),
        "parquet" => bail!("parquet support not implemented yet"),
        other => bail!("unsupported input extension {other:?} (expected .geojson/.json, .csv, .parquet)"),
    }
}

fn json_value_to_string(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::Null => None,
        serde_json::Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

fn read_geojson(path: &Path) -> Result<Dataset> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    let gj: geojson::GeoJson = text
        .parse()
        .with_context(|| format!("parsing {} as GeoJSON", path.display()))?;
    let fc = match gj {
        geojson::GeoJson::FeatureCollection(fc) => fc,
        _ => bail!("expected a GeoJSON FeatureCollection"),
    };

    // Column order: union of property keys in first-seen order
    // (serde_json's preserve_order feature keeps each feature's key order).
    let mut columns: Vec<String> = Vec::new();
    for f in &fc.features {
        if let Some(props) = &f.properties {
            for k in props.keys() {
                if !columns.iter().any(|c| c == k) {
                    columns.push(k.clone());
                }
            }
        }
    }

    let mut geoms = Vec::with_capacity(fc.features.len());
    let mut rows = Vec::with_capacity(fc.features.len());
    for f in fc.features {
        let geom = match &f.geometry {
            Some(g) => Some(
                Geometry::<f64>::try_from(g.value.clone())
                    .map_err(|e| anyhow!("invalid geometry: {e}"))?,
            ),
            None => None,
        };
        geoms.push(geom);
        let props = f.properties.unwrap_or_default();
        rows.push(
            columns
                .iter()
                .map(|c| props.get(c).and_then(json_value_to_string))
                .collect(),
        );
    }
    Ok(Dataset { geoms, columns, rows })
}

fn read_csv(path: &Path, lat_col: Option<&str>, lng_col: Option<&str>) -> Result<Dataset> {
    let mut reader = csv::Reader::from_path(path)
        .with_context(|| format!("reading {}", path.display()))?;
    let headers: Vec<String> = reader.headers()?.iter().map(|h| h.to_string()).collect();
    let (lat_idx, lng_idx) = detect_latlng(&headers, lat_col, lng_col)?;

    let columns: Vec<String> = headers
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != lat_idx && *i != lng_idx)
        .map(|(_, h)| h.clone())
        .collect();

    let mut geoms = Vec::new();
    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record?;
        let lat: Option<f64> = record.get(lat_idx).and_then(|v| v.trim().parse().ok());
        let lng: Option<f64> = record.get(lng_idx).and_then(|v| v.trim().parse().ok());
        geoms.push(match (lat, lng) {
            (Some(lat), Some(lng)) => Some(Geometry::Point(geo::Point::new(lng, lat))),
            _ => None,
        });
        rows.push(
            headers
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != lat_idx && *i != lng_idx)
                .map(|(i, _)| {
                    record
                        .get(i)
                        .map(str::to_string)
                        .filter(|s| !s.is_empty())
                })
                .collect(),
        );
    }
    Ok(Dataset { geoms, columns, rows })
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --test input`
Expected: 6 passed. Also run `cargo test` (all) — 19 passed total.

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs src/input.rs tests/input.rs tests/fixtures/points.geojson tests/fixtures/polys.geojson tests/fixtures/points.csv
git commit -m "feat: GeoJSON and CSV readers with lat/lng autodetection"
```

---

### Task 4: Parquet readers — plain, GeoParquet WKB, CRS reprojection

**Files:**
- Modify: `src/input.rs` (replace the `"parquet" => bail!` arm)
- Create: `tests/fixtures/make_fixtures.py`, and the three generated committed fixtures `tests/fixtures/points_plain.parquet`, `tests/fixtures/points_geo.parquet`, `tests/fixtures/points_3857.parquet`
- Test: `tests/input.rs` (append)

**Interfaces:**
- Consumes: `Dataset`, `detect_latlng` (Task 3).
- Produces: `read_input` now handles `.parquet`. No new public items.

- [ ] **Step 1: Write and run the fixture generator**

`tests/fixtures/make_fixtures.py`:

```python
# Generates the parquet fixtures. Run from the repo root:
#   uv run --with geopandas --with pyarrow python tests/fixtures/make_fixtures.py
# Artifacts are committed; re-run only when fixtures must change.
import geopandas as gpd
import pandas as pd
from shapely.geometry import Point

FIX = "tests/fixtures"

plain = pd.DataFrame(
    {"name": ["P1", "P2"], "lat": [6.5, 6.6], "lon": [3.4, 3.5], "pop": [10, 20]}
)
plain.to_parquet(f"{FIX}/points_plain.parquet", index=False)

gdf = gpd.GeoDataFrame(
    {"name": ["G1", None], "pop": [1, 2]},
    geometry=[Point(3.4, 6.5), None],
    crs="EPSG:4326",
)
gdf.to_parquet(f"{FIX}/points_geo.parquet", index=False)

gdf3857 = gpd.GeoDataFrame(
    {"name": ["M1"]},
    geometry=[Point(111319.49079327357, 111325.14286638486)],  # ~ (1.0, 1.0) deg
    crs="EPSG:3857",
)
gdf3857.to_parquet(f"{FIX}/points_3857.parquet", index=False)
print("fixtures written")
```

Run: `uv run --with geopandas --with pyarrow python tests/fixtures/make_fixtures.py`
Expected: `fixtures written`, three `.parquet` files in `tests/fixtures/`.

- [ ] **Step 2: Write the failing tests**

Append to `tests/input.rs`:

```rust
#[test]
fn plain_parquet_uses_latlng_columns() {
    let ds = read_input(&fixture("points_plain.parquet"), None, None).unwrap();
    assert_eq!(ds.columns, vec!["name", "pop"]);
    assert_eq!(ds.geoms.len(), 2);
    let p = match &ds.geoms[0] {
        Some(geo::Geometry::Point(p)) => p,
        other => panic!("expected point, got {other:?}"),
    };
    assert_eq!((p.y(), p.x()), (6.5, 3.4));
    assert_eq!(ds.rows[0], vec![Some("P1".into()), Some("10".into())]);
}

#[test]
fn geoparquet_decodes_wkb_and_nulls() {
    let ds = read_input(&fixture("points_geo.parquet"), None, None).unwrap();
    assert_eq!(ds.columns, vec!["name", "pop"]);
    assert!(matches!(ds.geoms[0], Some(geo::Geometry::Point(_))));
    assert!(ds.geoms[1].is_none()); // null geometry row
    assert_eq!(ds.rows[1][0], None); // null attribute -> None
}

#[test]
fn geoparquet_3857_is_reprojected_to_4326() {
    let ds = read_input(&fixture("points_3857.parquet"), None, None).unwrap();
    let p = match &ds.geoms[0] {
        Some(geo::Geometry::Point(p)) => p,
        other => panic!("expected point, got {other:?}"),
    };
    assert!((p.x() - 1.0).abs() < 1e-6, "lng {}", p.x());
    assert!((p.y() - 1.0).abs() < 1e-6, "lat {}", p.y());
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --test input`
Expected: the three new tests FAIL with "parquet support not implemented yet".

- [ ] **Step 4: Implement**

In `src/input.rs`, replace the `"parquet" => bail!(...)` arm with `"parquet" => read_parquet(path, lat_col, lng_col),` and append:

```rust
use arrow::array::{Array, BinaryArray, LargeBinaryArray};
use arrow::record_batch::RecordBatch;
use geozero::{wkb::Wkb, ToGeo};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

fn read_parquet(path: &Path, lat_col: Option<&str>, lng_col: Option<&str>) -> Result<Dataset> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("reading {}", path.display()))?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;

    // GeoParquet: file-level KV metadata key "geo" names the geometry column.
    let geo_meta: Option<serde_json::Value> = builder
        .metadata()
        .file_metadata()
        .key_value_metadata()
        .and_then(|kvs| kvs.iter().find(|k| k.key == "geo"))
        .and_then(|k| k.value.as_ref())
        .and_then(|v| serde_json::from_str(v).ok());
    let primary: Option<String> = geo_meta
        .as_ref()
        .and_then(|m| m["primary_column"].as_str())
        .map(str::to_string);

    // CRS: GeoParquet stores PROJJSON per column; absent means OGC:CRS84
    // (lon/lat, equivalent to 4326 for our purposes). We support EPSG codes
    // via crs-definitions; anything else is fatal.
    let epsg: Option<i64> = primary.as_ref().and_then(|p| {
        let crs = &geo_meta.as_ref()?["columns"][p.as_str()]["crs"];
        if crs.is_null() {
            None
        } else {
            crs["id"]["code"].as_i64().or(Some(-1)) // -1 = present but not an EPSG id
        }
    });

    let batches: Vec<RecordBatch> = builder.build()?.collect::<std::result::Result<_, _>>()?;
    let schema = batches
        .first()
        .map(|b| b.schema())
        .ok_or_else(|| anyhow!("parquet file has no rows"))?;
    let all_columns: Vec<String> = schema.fields().iter().map(|f| f.name().clone()).collect();

    match primary {
        Some(geom_col) => {
            let attr_columns: Vec<String> = all_columns
                .iter()
                .filter(|c| **c != geom_col)
                .cloned()
                .collect();
            let mut geoms = Vec::new();
            let mut rows = Vec::new();
            for batch in &batches {
                let gi = batch.schema().index_of(&geom_col)?;
                let col = batch.column(gi);
                for row in 0..batch.num_rows() {
                    geoms.push(wkb_at(col.as_ref(), row)?);
                    rows.push(stringify_row(batch, row, &attr_columns)?);
                }
            }
            match epsg {
                None | Some(4326) => {}
                Some(-1) => bail!(
                    "GeoParquet CRS is not identified by an EPSG code; reproject the file to EPSG:4326 first"
                ),
                Some(code) => reproject(&mut geoms, code)?,
            }
            Ok(Dataset { geoms, columns: attr_columns, rows })
        }
        None => {
            let (lat_idx, lng_idx) = detect_latlng(&all_columns, lat_col, lng_col)?;
            let attr_columns: Vec<String> = all_columns
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != lat_idx && *i != lng_idx)
                .map(|(_, c)| c.clone())
                .collect();
            let mut geoms = Vec::new();
            let mut rows = Vec::new();
            for batch in &batches {
                for row in 0..batch.num_rows() {
                    let lat = float_at(batch, lat_idx, row);
                    let lng = float_at(batch, lng_idx, row);
                    geoms.push(match (lat, lng) {
                        (Some(lat), Some(lng)) => {
                            Some(Geometry::Point(geo::Point::new(lng, lat)))
                        }
                        _ => None,
                    });
                    rows.push(stringify_row(batch, row, &attr_columns)?);
                }
            }
            Ok(Dataset { geoms, columns: attr_columns, rows })
        }
    }
}

fn wkb_at(col: &dyn Array, row: usize) -> Result<Option<Geometry<f64>>> {
    if col.is_null(row) {
        return Ok(None);
    }
    let bytes: Vec<u8> = if let Some(b) = col.as_any().downcast_ref::<BinaryArray>() {
        b.value(row).to_vec()
    } else if let Some(b) = col.as_any().downcast_ref::<LargeBinaryArray>() {
        b.value(row).to_vec()
    } else {
        bail!("geometry column is not WKB-encoded binary (found {:?})", col.data_type());
    };
    let geom = Wkb(bytes)
        .to_geo()
        .map_err(|e| anyhow!("decoding WKB geometry at row {}: {e}", row + 1))?;
    Ok(Some(geom))
}

fn float_at(batch: &RecordBatch, col: usize, row: usize) -> Option<f64> {
    let col = batch.column(col);
    if col.is_null(row) {
        return None;
    }
    arrow::util::display::array_value_to_string(col, row)
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

fn stringify_row(batch: &RecordBatch, row: usize, attr_columns: &[String]) -> Result<Vec<Option<String>>> {
    attr_columns
        .iter()
        .map(|name| {
            let i = batch.schema().index_of(name)?;
            let col = batch.column(i);
            if col.is_null(row) {
                Ok(None)
            } else {
                Ok(Some(arrow::util::display::array_value_to_string(col, row)?))
            }
        })
        .collect()
}

/// Reproject in place from `code` to EPSG:4326 via proj4rs. Geographic
/// sources feed radians into proj4rs; geographic outputs come back in
/// radians and are converted to degrees.
fn reproject(geoms: &mut [Option<Geometry<f64>>], code: i64) -> Result<()> {
    use geo::MapCoordsInPlace;
    let code_u16: u16 = code
        .try_into()
        .map_err(|_| anyhow!("unsupported CRS code {code}; reproject to EPSG:4326 first"))?;
    let src_def = crs_definitions::from_code(code_u16)
        .ok_or_else(|| anyhow!("unknown EPSG code {code}; reproject to EPSG:4326 first"))?;
    let dst_def = crs_definitions::from_code(4326).expect("4326 definition exists");
    let src = proj4rs::Proj::from_proj_string(src_def.proj4)
        .map_err(|e| anyhow!("EPSG:{code}: {e}"))?;
    let dst = proj4rs::Proj::from_proj_string(dst_def.proj4)
        .map_err(|e| anyhow!("EPSG:4326: {e}"))?;
    let src_is_geographic = src_def.proj4.contains("+proj=longlat");

    let mut failed = false;
    for geom in geoms.iter_mut().flatten() {
        geom.map_coords_in_place(|c| {
            let mut pt = if src_is_geographic {
                (c.x.to_radians(), c.y.to_radians(), 0.0)
            } else {
                (c.x, c.y, 0.0)
            };
            match proj4rs::transform::transform(&src, &dst, &mut pt) {
                Ok(()) => geo::Coord { x: pt.0.to_degrees(), y: pt.1.to_degrees() },
                Err(_) => {
                    failed = true;
                    c
                }
            }
        });
    }
    if failed {
        bail!("coordinate transform from EPSG:{code} failed for at least one point");
    }
    Ok(())
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --test input`
Expected: 9 passed. `cargo test` — 22 passed total.

- [ ] **Step 6: Commit**

```bash
git add src/input.rs tests/input.rs tests/fixtures/make_fixtures.py tests/fixtures/points_plain.parquet tests/fixtures/points_geo.parquet tests/fixtures/points_3857.parquet
git commit -m "feat: parquet readers — plain lat/lng, GeoParquet WKB, CRS reprojection"
```

---

### Task 5: CSV output, CLI wiring, end-to-end tests, README

**Files:**
- Create: `src/output.rs`, `tests/cli.rs`, `tests/fixtures/points-entities.golden.csv`, `README.md`
- Modify: `src/lib.rs` (add `pub mod output;`), `src/main.rs` (replace stub)

**Interfaces:**
- Consumes: `input::{read_input, Dataset}`, `convert::{detect_family, geopoint, geoshape, make_labels, sanitize_columns, Family, DEFAULT_MAX_VERTICES}`.
- Produces: the `odk-locations` binary per the spec's CLI; `output::write_entities` + `output::Summary`.

- [ ] **Step 1: Write the failing CLI tests**

`tests/fixtures/points-entities.golden.csv` (exact bytes; final newline present):

```csv
label,geometry,pop,active
Clinic A,6.5 3.4 0 0,1200,true
Clinic B,6.6 3.5 0 0,800,false
```

(Row 3 of `points.geojson` has null geometry → skipped; `name` is consumed as the default label column and therefore not repeated as a property.)

`tests/cli.rs`:

```rust
use assert_cmd::Command;
use predicates::prelude::*;
use std::path::Path;

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

#[test]
fn geojson_to_entities_csv_matches_golden() {
    let dir = tempdir();
    let out = dir.join("out.csv");
    Command::cargo_bin("odk-locations")
        .unwrap()
        .arg(fixture("points.geojson"))
        .arg("-o")
        .arg(&out)
        .assert()
        .success()
        .stderr(predicate::str::contains("2 written"))
        .stderr(predicate::str::contains("1 skipped"));
    let got = std::fs::read_to_string(&out).unwrap();
    let want = std::fs::read_to_string(fixture("points-entities.golden.csv")).unwrap();
    assert_eq!(got, want);
}

#[test]
fn default_output_path_is_input_stem_entities() {
    let dir = tempdir();
    let input = dir.join("sites.geojson");
    std::fs::copy(fixture("points.geojson"), &input).unwrap();
    Command::cargo_bin("odk-locations")
        .unwrap()
        .arg(&input)
        .current_dir(&dir)
        .assert()
        .success();
    assert!(dir.join("sites-entities.csv").exists());
}

#[test]
fn polygon_boundary_default_and_centroid_flag() {
    let dir = tempdir();
    let out = dir.join("out.csv");
    Command::cargo_bin("odk-locations")
        .unwrap()
        .arg(fixture("polys.geojson"))
        .arg("-o")
        .arg(&out)
        .assert()
        .success();
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.contains(";"), "boundary default should emit geoshape rings");

    Command::cargo_bin("odk-locations")
        .unwrap()
        .arg(fixture("polys.geojson"))
        .arg("--geometry")
        .arg("centroid")
        .arg("-o")
        .arg(&out)
        .assert()
        .success();
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(!text.contains(";"), "centroid mode must emit geopoints only");
}

#[test]
fn label_column_flag_and_unknown_label_column_error() {
    let dir = tempdir();
    let out = dir.join("out.csv");
    Command::cargo_bin("odk-locations")
        .unwrap()
        .arg(fixture("points.geojson"))
        .arg("--label-column")
        .arg("pop")
        .arg("-o")
        .arg(&out)
        .assert()
        .success();
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.lines().nth(1).unwrap().starts_with("1200,"));
    // 'name' returns to the property columns when it is not the label
    assert!(text.lines().next().unwrap().contains("name"));

    Command::cargo_bin("odk-locations")
        .unwrap()
        .arg(fixture("points.geojson"))
        .arg("--label-column")
        .arg("nope")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("nope"));
}

#[test]
fn csv_and_parquet_inputs_work_end_to_end() {
    let dir = tempdir();
    for f in ["points.csv", "points_plain.parquet", "points_geo.parquet"] {
        let out = dir.join(format!("{f}.out.csv"));
        Command::cargo_bin("odk-locations")
            .unwrap()
            .arg(fixture(f))
            .arg("-o")
            .arg(&out)
            .assert()
            .success();
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(text.starts_with("label,geometry"), "{f}: {text}");
    }
}

#[test]
fn all_rows_skipped_is_fatal() {
    let dir = tempdir();
    let input = dir.join("empty.geojson");
    std::fs::write(
        &input,
        r#"{"type":"FeatureCollection","features":[{"type":"Feature","geometry":null,"properties":{"name":"x"}}]}"#,
    )
    .unwrap();
    Command::cargo_bin("odk-locations")
        .unwrap()
        .arg(&input)
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("geometry"));
}

fn tempdir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("odk-locations-test-{}", std::process::id()))
        .join(format!("{:x}", rand_suffix()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn rand_suffix() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test cli`
Expected: every test FAILS (binary prints "not implemented yet", exit 1).

- [ ] **Step 3: Implement output.rs**

Add `pub mod output;` to `src/lib.rs`. Create `src/output.rs`:

```rust
//! Entity CSV writer and run summary.

use anyhow::{Context, Result};
use std::path::Path;

pub struct Summary {
    pub written: usize,
    pub skipped: usize,
    pub renamed: Vec<(String, String)>,
}

/// Write the entity CSV: header `label,geometry,<sanitized attrs…>`, one row
/// per feature with a geometry string. Rows whose geometry is None are
/// skipped and counted.
pub fn write_entities(
    path: &Path,
    labels: &[String],
    geometries: &[Option<String>],
    sanitized_columns: &[String],
    original_columns: &[String],
    rows: &[Vec<Option<String>>],
) -> Result<Summary> {
    let mut writer = csv::Writer::from_path(path)
        .with_context(|| format!("writing {}", path.display()))?;
    let mut header: Vec<&str> = vec!["label", "geometry"];
    header.extend(sanitized_columns.iter().map(String::as_str));
    writer.write_record(&header)?;

    let mut written = 0;
    let mut skipped = 0;
    for (i, geom) in geometries.iter().enumerate() {
        let Some(geom) = geom else {
            skipped += 1;
            continue;
        };
        let mut record: Vec<&str> = vec![labels[i].as_str(), geom.as_str()];
        for v in &rows[i] {
            record.push(v.as_deref().unwrap_or(""));
        }
        writer.write_record(&record)?;
        written += 1;
    }
    writer.flush()?;

    let renamed = original_columns
        .iter()
        .zip(sanitized_columns.iter())
        .filter(|(o, s)| o != s)
        .map(|(o, s)| (o.clone(), s.clone()))
        .collect();
    Ok(Summary { written, skipped, renamed })
}
```

- [ ] **Step 4: Implement main.rs**

Replace `src/main.rs`:

```rust
use anyhow::{bail, Result};
use clap::Parser;
use odk_locations::convert::{
    detect_family, geopoint, geoshape, make_labels, sanitize_columns, Family,
    DEFAULT_MAX_VERTICES,
};
use odk_locations::input::read_input;
use odk_locations::output::write_entities;
use std::path::PathBuf;

const NAME_LIKE: [&str; 3] = ["name", "title", "label"];

/// Convert GeoJSON / (Geo)Parquet / CSV point-or-polygon data into an ODK
/// entity CSV (label, geometry in ODK geopoint/geoshape format, all
/// attributes as properties). Ready for ODK Central's bulk entity upload.
#[derive(Parser)]
#[command(name = "odk-locations", version)]
struct Cli {
    /// Input file (.geojson/.json, .csv, .parquet)
    input: PathBuf,
    /// Output CSV path [default: <input-stem>-entities.csv]
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Attribute column used for entity labels [default: name/title/label,
    /// else feature-<n>]
    #[arg(long)]
    label_column: Option<String>,
    /// Polygon geometry mode (points always export as geopoints)
    #[arg(long, value_parser = ["centroid", "boundary"], default_value = "boundary")]
    geometry: String,
    /// Geoshape ring vertex cap
    #[arg(long, default_value_t = DEFAULT_MAX_VERTICES)]
    max_vertices: usize,
    /// Latitude column (CSV/plain-parquet inputs without geometry)
    #[arg(long)]
    lat_column: Option<String>,
    /// Longitude column (CSV/plain-parquet inputs without geometry)
    #[arg(long)]
    lng_column: Option<String>,
    /// Suppress the summary printed to stderr
    #[arg(long)]
    quiet: bool,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let dataset = read_input(
        &cli.input,
        cli.lat_column.as_deref(),
        cli.lng_column.as_deref(),
    )?;
    let family = detect_family(&dataset.geoms).map_err(anyhow::Error::msg)?;

    // Label column: explicit flag (must exist), else first name-like column.
    let label_idx: Option<usize> = match &cli.label_column {
        Some(name) => Some(
            dataset
                .columns
                .iter()
                .position(|c| c == name)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "label column {name:?} not found (columns: {:?})",
                        dataset.columns
                    )
                })?,
        ),
        None => dataset.columns.iter().position(|c| {
            NAME_LIKE.contains(&c.to_ascii_lowercase().as_str())
        }),
    };
    let label_values: Option<Vec<Option<String>>> =
        label_idx.map(|i| dataset.rows.iter().map(|r| r[i].clone()).collect());
    let labels = make_labels(label_values.as_deref(), dataset.geoms.len());

    // The label column is consumed — drop it from the property columns.
    let (prop_columns, prop_rows): (Vec<String>, Vec<Vec<Option<String>>>) = match label_idx {
        Some(li) => (
            dataset
                .columns
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != li)
                .map(|(_, c)| c.clone())
                .collect(),
            dataset
                .rows
                .iter()
                .map(|r| {
                    r.iter()
                        .enumerate()
                        .filter(|(i, _)| *i != li)
                        .map(|(_, v)| v.clone())
                        .collect()
                })
                .collect(),
        ),
        None => (dataset.columns.clone(), dataset.rows.clone()),
    };
    let sanitized = sanitize_columns(&prop_columns);

    let geometries: Vec<Option<String>> = dataset
        .geoms
        .iter()
        .map(|g| {
            g.as_ref().and_then(|g| match (family, cli.geometry.as_str()) {
                (Family::Polygon, "boundary") => geoshape(g, cli.max_vertices),
                _ => geopoint(g),
            })
        })
        .collect();

    if geometries.iter().all(Option::is_none) {
        bail!("no exportable features: every row has a missing or empty geometry");
    }

    let output = cli.output.clone().unwrap_or_else(|| {
        let stem = cli.input.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
        cli.input.with_file_name(format!("{stem}-entities.csv"))
    });
    let summary = write_entities(
        &output,
        &labels,
        &geometries,
        &sanitized,
        &prop_columns,
        &prop_rows,
    )?;

    if !cli.quiet {
        eprintln!(
            "{} written, {} skipped (empty geometry) -> {}",
            summary.written,
            summary.skipped,
            output.display()
        );
        for (old, new) in &summary.renamed {
            eprintln!("renamed column {old:?} -> {new:?}");
        }
    }
    Ok(())
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test`
Expected: all pass (22 unit/integration + 7 CLI = 29). If the golden file mismatches on float formatting, inspect the actual output — the golden must match Rust `Display` output exactly; fix the golden only if the actual output is spec-correct.

- [ ] **Step 6: Write README.md**

```markdown
# odk-locations

Convert GeoJSON / GeoParquet / parquet / CSV point-or-polygon data into an
ODK entity CSV — `label`, `geometry` (ODK geopoint/geoshape strings), and
every attribute as an entity property. The output is directly usable by
ODK Central's bulk entity upload.

## Usage

    odk-locations INPUT [-o entities.csv]
        [--label-column NAME]           # default: name/title/label, else feature-<n>
        [--geometry centroid|boundary]  # polygons only; default boundary
        [--max-vertices 500]
        [--lat-column LAT --lng-column LNG]  # CSV/parquet without geometry
        [--quiet]

Examples:

    odk-locations sites.geojson
    odk-locations sites.csv --lat-column Latitude --lng-column LON
    odk-locations admin_areas.parquet --geometry centroid --label-column adm2_name

## Notes

- Points export as ODK geopoints ("lat lng 0 0"). Polygons export as
  geoshapes (largest part's exterior ring, holes dropped, simplified to
  ≤500 vertices) or geopoints with `--geometry centroid`.
- Labels are deterministic: label-column value, else `feature-<n>`;
  duplicates get ` (2)`, ` (3)`….
- Property names are sanitized to ODK rules; renames are reported on stderr.
- CRS: 4326 assumed when undeclared; GeoParquet with an EPSG-coded CRS is
  reprojected (pure Rust); anything else must be reprojected upstream.
- Line/mixed-geometry inputs are rejected.

## Build

    cargo build --release   # target/release/odk-locations (static binary)
    cargo test
```

- [ ] **Step 7: Commit**

```bash
git add src/lib.rs src/main.rs src/output.rs tests/cli.rs tests/fixtures/points-entities.golden.csv README.md
git commit -m "feat: CLI wiring, entity CSV writer, end-to-end tests, README"
```

---

### Task 6: Polish gates — fmt, clippy, release build

**Files:** none new (fixes only if gates fail).

- [ ] **Step 1: Run the gates**

```bash
cargo fmt          # then `git diff --stat` — commit formatting if it changed anything
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
./target/release/odk-locations tests/fixtures/points.geojson -o /tmp/smoke.csv && cat /tmp/smoke.csv
```

Expected: fmt idempotent (or a formatting-only diff), clippy clean, all tests pass, release binary converts the fixture.

- [ ] **Step 2: Fix anything the gates flag**

Clippy/fmt findings get minimal mechanical fixes; re-run the failing gate after each fix. No behavior changes without a covering test.

- [ ] **Step 3: Commit**

```bash
git add -u
git commit -m "chore: fmt + clippy clean, release build verified"
```

(`git add -u` is safe here — this repo is fresh with no untracked clutter; only tracked files change.)
