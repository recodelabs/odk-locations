use odk_locations::input::{detect_latlng, read_input, Dataset};
use std::path::Path;

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn geojson_points_columns_rows_and_nulls() {
    let ds: Dataset = read_input(&fixture("points.geojson"), None, None).unwrap();
    assert_eq!(ds.columns, vec!["name", "pop", "active"]);
    assert_eq!(ds.geoms.len(), 3);
    assert!(ds.geoms[0].is_some());
    assert!(ds.geoms[2].is_none());
    assert_eq!(
        ds.rows[0],
        vec![
            Some("Clinic A".into()),
            Some("1200".into()),
            Some("true".into())
        ]
    );
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
    let err = detect_latlng(&cols, Some("nope"), Some("b"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("nope"), "{err}");
}

#[test]
fn unknown_extension_fails() {
    let err = read_input(Path::new("data.shp"), None, None)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains(".shp") || err.to_lowercase().contains("unsupported"),
        "{err}"
    );
}

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
