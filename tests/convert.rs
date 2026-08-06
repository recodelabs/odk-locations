use odk_locations::convert::{make_labels, sanitize_columns};

fn s(v: &str) -> Option<String> {
    Some(v.to_string())
}

#[test]
fn labels_from_column_with_fallback_and_dedupe() {
    let vals = [s("A"), s("A"), s(""), None, s("A")];
    assert_eq!(
        make_labels(Some(&vals[..]), 5),
        vec!["A", "A (2)", "feature-3", "feature-4", "A (3)"]
    );
}

#[test]
fn labels_without_column_are_positional() {
    assert_eq!(
        make_labels(None, 3),
        vec!["feature-1", "feature-2", "feature-3"]
    );
}

#[test]
fn labels_trim_whitespace() {
    let vals = [s("  Clinic  "), s("   ")];
    assert_eq!(make_labels(Some(&vals[..]), 2), vec!["Clinic", "feature-2"]);
}

#[test]
fn label_dedupe_suffix_cannot_collide_with_literal_value() {
    // PR-185 deferred bug #1: literal "A (2)" must not collide with a
    // generated suffix. Suffixes are registered, so the collision resolves
    // deterministically.
    let vals = [s("A"), s("A"), s("A (2)")];
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
    assert_eq!(
        sanitize_columns(&cols),
        vec!["pop_2024", "r_gion_name", "a.b-c"]
    );
}

#[test]
fn sanitize_reserved_digit_start_and_collisions() {
    let cols: Vec<String> = ["label", "name", "geometry", "2024", "a b", "a_b", "__x"]
        .iter()
        .map(|c| c.to_string())
        .collect();
    assert_eq!(
        sanitize_columns(&cols),
        vec![
            "label_",
            "name_",
            "geometry_",
            "_2024",
            "a_b",
            "a_b_2",
            "_x"
        ]
    );
}

#[test]
fn sanitize_collision_suffix_never_starts_with_double_underscore() {
    // Review finding: "#" and "%" both sanitize to "_"; the naive suffix
    // loop would emit "__2", which starts with the reserved "__" prefix
    // ODK Central rejects. The re-collapsed suffix must land on "_2".
    let cols: Vec<String> = ["#", "%"].iter().map(|c| c.to_string()).collect();
    let out = sanitize_columns(&cols);
    assert_eq!(out, vec!["_", "_2"]);
    let set: std::collections::HashSet<_> = out.iter().collect();
    assert_eq!(set.len(), 2, "outputs must be unique");
    for name in &out {
        assert!(
            !name.starts_with("__"),
            "{name:?} starts with reserved __ prefix"
        );
    }
}

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
        LineString::from(vec![
            (0.0, 0.0),
            (4.0, 0.0),
            (4.0, 4.0),
            (0.0, 4.0),
            (0.0, 0.0),
        ]),
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
