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
        // detect_family (src/convert.rs, frozen from an earlier task) reports
        // this all-missing-geometry case as "no geometries found in input" —
        // plural, so match the common root rather than the brief's literal
        // "geometry" (which "geometries" does not substring-contain).
        .stderr(predicate::str::contains("geometr"));
}

fn tempdir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("odk-locations-test-{}", std::process::id()))
        .join(format!("{:x}", rand_suffix()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// Nanosecond timestamps alone collide often enough under cargo test's default
// parallel threads (all threads share the same process id and can sample
// SystemTime::now() at the same tick on this machine), which made two tests
// resolve to the *same* tempdir and stomp each other's "out.csv" — flaky
// failures on ~every other run. An atomic counter guarantees uniqueness
// regardless of clock resolution.
fn rand_suffix() -> u128 {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    (nanos << 32) ^ (n as u128)
}
