# odk-locations: ODK entity-CSV converter CLI — design

**Date:** 2026-08-06
**Status:** approved (conversation, 2026-08-06; revised to Rust same day)

## Goal

A standalone CLI, shipped as a **single static Rust binary**, that converts
a GeoJSON / GeoParquet / plain-parquet / CSV file of point or polygon
features into an **ODK entity CSV**: a `label` column, a `geometry` column
in ODK geopoint/geoshape format, and every input attribute passed through
as an entity property. Output is directly usable by ODK Central's bulk
entity upload and matches the property/geometry shape a future Ona
uploader will consume.

This lives **outside the Pixel repo** (`~/github/odk-locations`) so it can
be used — and distributed — without managing anything in Pixel and without
Python/uv on the target machine. The geometry/label semantics are a
**reimplementation** of the Pixel worker logic merged in PR #185, verified
against test vectors ported from that PR's suite (the two copies share a
spec and tests, not code).

## Non-goals

- No uploading/pushing to Ona or ODK Central (converter only).
- No line/mixed-geometry support.
- No WKT output (ODK tooling doesn't consume it).
- No C dependencies (no libproj/GDAL) — pure-Rust crates only, so plain
  `cargo build --release` cross-compiles to static binaries.

## Layout

```
odk-locations/
  Cargo.toml                 # bin name: odk-locations, edition 2021
  src/
    main.rs                  # CLI arg parsing (clap), wiring, exit codes
    input.rs                 # per-format readers -> Vec<Feature{geom, attrs}>
    convert.rs               # geometry strings, labels, sanitization (pure)
    output.rs                # entity CSV writer + summary
  tests/
    convert.rs               # unit tests: ported PR-185 vectors
    cli.rs                   # end-to-end golden-file tests (assert_cmd)
    fixtures/                # small geojson/csv/parquet inputs + expected CSVs
  docs/specs/
  README.md
```

Crates: `clap` (derive), `geojson`, `geo` (InteriorPoint/Area/Simplify),
`csv`, `parquet` + `arrow` (column access), `wkb` or `geozero` (decode
GeoParquet's WKB geometry column), `proj4rs` (CRS transforms),
`serde_json`; dev: `assert_cmd`, `predicates`.

## CLI

```
odk-locations INPUT [-o entities.csv]
    [--label-column NAME]          # default: name-like column (name/title/label,
                                   #   case-insensitive), else feature-<n>
    [--geometry centroid|boundary] # polygons only; default boundary;
                                   #   points always geopoint
    [--max-vertices 500]
    [--lat-column LAT --lng-column LNG]  # CSV/plain-parquet without geometry:
                                   #   build points (auto-detects lat/latitude/y,
                                   #   lng/lon/long/longitude/x)
    [--quiet]
```

`-o` defaults to `<input-stem>-entities.csv`.

## Input handling

- `.geojson`/`.json` → `geojson` crate (FeatureCollection; attrs from
  `properties`).
- `.parquet` → read with `parquet`/`arrow`; if GeoParquet metadata (`geo`
  file-level KV) names a geometry column, decode its WKB values via
  `geozero`/`wkb` into `geo` types — this deliberately avoids the
  still-churning geoarrow-rs API. Without geo metadata, fall back to
  lat/lng columns.
- `.csv` → `csv` crate + lat/lng columns (WKT-in-CSV not supported).
- Geometry family decided from the data: all Point/MultiPoint → point;
  all Polygon/MultiPolygon → polygon; anything else fails, naming the
  offending geometry types and up to 5 example row indices.
- **CRS policy:** GeoJSON is 4326 by RFC 7946, CSV/plain-parquet lat/lng
  are 4326 by convention, and GeoParquet without a `crs` field defaults to
  OGC:CRS84 — all treated as 4326 silently. A GeoParquet CRS declared with
  an EPSG code other than 4326 → transform via `proj4rs` (covers 3857 and
  other common projected CRSs); a CRS not identified by an EPSG code →
  fatal error telling the user to reproject first.

## Conversion rules (semantics identical to Pixel PR #185)

- geopoint `"{lat} {lng} 0 0"` via interior/representative point (always
  inside polygons; MultiPoint picks a member point).
- geoshape: largest-area part of a (Multi)Polygon, exterior ring only
  (holes dropped), closed ring (first vertex repeated last), `"lat lng 0 0"`
  tuples joined by `;`, Douglas-Peucker-simplified with growing tolerance
  (×4 per round, capped iterations) until under `--max-vertices`.
- Labels: label column value, trimmed; empty/null → `feature-<n>`
  (1-based); duplicates deduped ` (2)`, ` (3)`… deterministically.
  The two PR-185 deferred label bugs are fixed here: generated dedupe
  suffixes are registered in the seen-set (a literal `"X (2)"` value cannot
  collide), and null detection covers all null-ish values uniformly.
- Rows with empty/missing geometry are skipped and counted; all-skipped is
  a fatal error.
- Float formatting: shortest round-trip representation (Rust's default
  `Display` for f64), which keeps coordinates lossless.

## Output

CSV (UTF-8, header row): `label`, `geometry`, then sanitized attribute
columns in input order. The label column is consumed by `label` and not
repeated as a property. Attribute names are sanitized to ODK property
rules: invalid chars → `_`; must not equal `label` or `name` (reserved by
Central) and must not start with `__`; post-sanitize collisions get `_2`,
`_3`… suffixes. Attribute values are written as strings; nulls → empty
string; numbers keep their input formatting where representable.

A summary (suppressed by `--quiet`) prints to stderr: rows written, rows
skipped for empty geometry, renamed columns (old → new), and the output
path.

## Errors

Fatal, with clear messages: unreadable input (underlying parser error
included), unsupported geometry family, missing/undetectable lat-lng
columns, unknown `--label-column`, unsupported CRS, all rows skipped.
Exit code 1; summary/warnings on stderr so stdout stays clean if `-o -`
(stdout output) is ever added later.

## Testing

- `tests/convert.rs` — unit tests porting the PR-185 worker vectors:
  point/MultiPoint geopoint, polygon centroid-inside, boundary closed-ring
  + hole-dropped, MultiPolygon largest-part, vertex-cap simplification,
  label column/fallback/dedupe determinism (incl. the literal-`"X (2)"`
  collision fix), empty-geometry skip + all-empty failure, name
  sanitization/collision/reserved cases.
- `tests/cli.rs` — `assert_cmd` end-to-end: GeoJSON → golden CSV,
  CSV lat/lng autodetect, plain-parquet lat/lng, GeoParquet WKB, non-4326
  reprojection, error exit codes + messages.
- CI (GitHub Actions when the repo gets a remote): `cargo fmt --check`,
  `cargo clippy -- -D warnings`, `cargo test`, release-build matrix
  (linux-musl, macos-arm64, windows) attaching binaries.
