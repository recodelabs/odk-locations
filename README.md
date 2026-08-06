# odk-locations

Convert GeoJSON / GeoParquet / parquet / CSV point-or-polygon data into an
ODK entity CSV — `label`, `geometry` (ODK geopoint/geoshape strings), and
every attribute as an entity property. The output is directly usable by
Ona Data or ODK Central's bulk entity upload.

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
- CRS: 4326 assumed when undeclared; OGC:CRS84 (the GeoParquet spec default,
  and what ogr2ogr/geopandas write when no target SRS is given) is treated as
  4326-equivalent with no reprojection. GeoParquet with an EPSG-coded CRS
  (numeric or string) is reprojected (pure Rust); anything else must be
  reprojected upstream.
- Line/mixed-geometry inputs are rejected.

## Build

    cargo build --release   # target/release/odk-locations (static binary)
    cargo test
