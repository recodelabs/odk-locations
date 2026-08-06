# odk-locations: ODK entity-CSV converter CLI — design

**Date:** 2026-08-06
**Status:** approved (conversation, 2026-08-06)

## Goal

A standalone, self-contained CLI that converts a GeoJSON / GeoParquet /
plain-parquet / CSV file of point or polygon features into an **ODK entity
CSV**: a `label` column, a `geometry` column in ODK geopoint/geoshape
format, and every input attribute passed through as an entity property.
Output is directly usable by ODK Central's bulk entity upload and matches
the property/geometry shape a future Ona uploader will consume.

This deliberately lives **outside the Pixel repo** (`~/github/odk-locations`)
so it can be used without managing anything in Pixel. The geometry/label
logic is a fresh copy (~80 lines) of the worker logic merged in Pixel
PR #185 — a later shared-library extraction supersedes both copies.

## Non-goals

- No uploading/pushing to Ona or ODK Central (converter only; the Ona
  quirk handling stays in Pixel until the shared library exists).
- No line/mixed-geometry support.
- No WKT output (ODK tooling doesn't consume it).
- No packaging/publishing — a single PEP 723 script run via `uv run`.

## Layout

```
odk-locations/
  odk_entity_export.py        # the CLI (PEP 723 inline deps)
  tests/test_odk_entity_export.py
  README.md
```

Inline script deps: `geopandas`, `shapely`, `pandas`, `pyarrow`.
Tests run with `uv run --with pytest pytest tests/` (script imported as a module).

## CLI

```
uv run odk_entity_export.py INPUT [-o entities.csv]
    [--label-column NAME]          # default: name-like column (name/title/label,
                                   #   case-insensitive), else feature-<n>
    [--geometry centroid|boundary] # polygons only; default boundary;
                                   #   points always geopoint
    [--max-vertices 500]
    [--lat-column LAT --lng-column LNG]  # CSV without geometry: build points
                                   #   (auto-detects lat/latitude/y, lng/lon/long/longitude/x)
    [--quiet]
```

`-o` defaults to `<input-stem>-entities.csv`.

## Input handling

- `.geojson`/`.json` → `gpd.read_file`; `.parquet` → `gpd.read_parquet`,
  falling back to `pd.read_parquet` + lat/lng columns if no geo metadata;
  `.csv` → `pd.read_csv` + lat/lng columns (WKT-in-CSV not supported).
- Geometry family decided from the frame: all-Point/MultiPoint → point;
  all-Polygon/MultiPolygon → polygon; anything else fails, naming the
  offending geometry types and up to 5 example row indices.
- CRS: if the frame carries a CRS other than EPSG:4326, reproject; no CRS
  → assume 4326 with a warning.

## Conversion rules (ported from Pixel worker `odk_export_activities.py`)

- geopoint `"{lat} {lng} 0 0"` via `representative_point()` (always inside
  polygons; MultiPoint picks a member).
- geoshape: largest-area part of a (Multi)Polygon, exterior ring only
  (holes dropped), closed ring, `"lat lng 0 0"` tuples joined by `;`,
  simplified with growing tolerance until under `--max-vertices`.
- Labels: label column value, stripped; empty → `feature-<n>` (1-based);
  duplicates deduped ` (2)`, ` (3)`… deterministically.
- Two PR-185 deferred minors are **fixed in this copy**:
  1. generated dedupe suffixes are registered in the seen-set, so a literal
     `"X (2)"` value cannot collide with a generated one;
  2. the empty-value check uses scalar `pd.isna` (catches `pd.NA`/`NaT`,
     not just `None`/float NaN).
- Rows with empty/missing geometry are skipped and counted; all-skipped is
  a fatal error.

## Output

CSV (UTF-8, header row): `label`, `geometry`, then sanitized attribute
columns in input order. Attribute names are sanitized to ODK property
rules: invalid chars → `_`; must not equal `label` or `name` (reserved by
Central) and must not start with `__`; post-sanitize collisions get `_2`,
`_3`… suffixes. Attribute values are written as strings; NaN/NA → empty.

A summary (suppressed by `--quiet`) prints: rows written, rows skipped for
empty geometry, renamed columns (old → new), and the output path.

## Errors

Fatal, with clear messages: unreadable input (underlying parser error
included), unsupported geometry family, missing/undetectable lat-lng
columns for CSV, unknown `--label-column`, all rows skipped. Exit code 1.

## Testing

`tests/test_odk_entity_export.py` (pytest), porting the PR-185 worker test
cases against the copied functions: point/MultiPoint geopoint, polygon
centroid-inside, boundary closed-ring + hole-dropped, MultiPolygon
largest-part, vertex-cap simplification, label column/fallback/dedupe
determinism (including the literal-`"X (2)"` collision fix and `pd.NA`
fallback fix), empty-geometry skip + all-empty failure. Plus CLI-level
tests: end-to-end GeoJSON → CSV golden output, CSV lat/lng autodetect,
column sanitization/collision, reserved-name renames, exit codes.
