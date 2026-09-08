//! Browser entry point (feature `wasm`, target `wasm32-unknown-unknown`).
//!
//! No file I/O and no CRS reprojection here — those live in `input`/`output`
//! (native CLI only) along with the parquet/arrow/proj4rs dependency chain.
//! The caller already has rows in memory (DuckDB-WASM, reading our own
//! EPSG:4326 registry) and hands them straight in as JSON; this is just
//! `convert`'s pure logic wrapped for JS, plus the same CSV assembly
//! `output::write_entities` does for the CLI, writing to an in-memory buffer
//! instead of a path.

use crate::convert::{
    detect_family, geopoint, geoshape, make_labels, sanitize_columns, Family,
    DEFAULT_MAX_VERTICES,
};
use geo::Geometry;
use std::collections::BTreeMap;
use wasm_bindgen::prelude::*;

#[derive(serde::Deserialize)]
struct InRow {
    label: Option<String>,
    /// A GeoJSON geometry object (`{"type": "Point", "coordinates": [...]}`),
    /// or null for a missing geometry.
    geometry: Option<serde_json::Value>,
    properties: BTreeMap<String, Option<String>>,
}

#[derive(serde::Deserialize)]
struct Options {
    #[serde(default = "default_geometry_mode")]
    geometry: String,
    #[serde(default = "default_max_vertices")]
    max_vertices: usize,
}
fn default_geometry_mode() -> String {
    "boundary".to_string()
}
fn default_max_vertices() -> usize {
    DEFAULT_MAX_VERTICES
}

#[derive(serde::Serialize)]
struct ConvertResult {
    csv: String,
    written: usize,
    skipped: usize,
    renamed: Vec<(String, String)>,
}

/// Convert filtered rows into an ODK entity CSV.
///
/// `rows_json`: `[{"label": string|null, "geometry": <GeoJSON geometry>|null,
/// "properties": {name: string|null, ...}}, ...]` — same row shape for every
/// feature, JS builds it straight from a DuckDB-WASM query.
///
/// `options_json`: `{"geometry": "boundary"|"centroid", "max_vertices": 500}`
/// (both optional, defaults shown).
///
/// Returns `{"csv": "...", "written": n, "skipped": n, "renamed": [[old,
/// new], ...]}` as a JSON string, or throws a plain error string (mixed
/// geometry families, no exportable features, bad JSON).
#[wasm_bindgen]
pub fn convert_to_entities(rows_json: &str, options_json: &str) -> Result<String, JsValue> {
    convert_inner(rows_json, options_json).map_err(|e| JsValue::from_str(&e))
}

fn convert_inner(rows_json: &str, options_json: &str) -> Result<String, String> {
    let rows: Vec<InRow> =
        serde_json::from_str(rows_json).map_err(|e| format!("parsing rows: {e}"))?;
    let opts: Options =
        serde_json::from_str(options_json).map_err(|e| format!("parsing options: {e}"))?;

    let geoms: Vec<Option<Geometry<f64>>> = rows
        .iter()
        .map(|r| {
            r.geometry
                .as_ref()
                .map(|v| {
                    let gj = geojson::Geometry::from_json_value(v.clone())
                        .map_err(|e| format!("invalid geometry: {e}"))?;
                    Geometry::<f64>::try_from(gj.value)
                        .map_err(|e| format!("invalid geometry: {e}"))
                })
                .transpose()
        })
        .collect::<Result<_, String>>()?;

    let family = detect_family(&geoms)?;

    let label_values: Vec<Option<String>> = rows.iter().map(|r| r.label.clone()).collect();
    let labels = make_labels(Some(&label_values), rows.len());

    // Column order: union of property keys in first-seen order.
    let mut columns: Vec<String> = Vec::new();
    for r in &rows {
        for k in r.properties.keys() {
            if !columns.iter().any(|c| c == k) {
                columns.push(k.clone());
            }
        }
    }
    let sanitized = sanitize_columns(&columns);
    let prop_rows: Vec<Vec<Option<String>>> = rows
        .iter()
        .map(|r| {
            columns
                .iter()
                .map(|c| r.properties.get(c).cloned().flatten())
                .collect()
        })
        .collect();

    let geometries: Vec<Option<String>> = geoms
        .iter()
        .map(|g| {
            g.as_ref().and_then(|g| match (family, opts.geometry.as_str()) {
                (Family::Polygon, "boundary") => geoshape(g, opts.max_vertices),
                _ => geopoint(g),
            })
        })
        .collect();

    if geometries.iter().all(Option::is_none) {
        return Err("no exportable features: every row has a missing or empty geometry".to_string());
    }

    let mut buf: Vec<u8> = Vec::new();
    let mut written = 0usize;
    let mut skipped = 0usize;
    {
        let mut writer = csv::Writer::from_writer(&mut buf);
        let mut header: Vec<&str> = vec!["label", "geometry"];
        header.extend(sanitized.iter().map(String::as_str));
        writer.write_record(&header).map_err(|e| e.to_string())?;
        for (i, geom) in geometries.iter().enumerate() {
            let Some(geom) = geom else {
                skipped += 1;
                continue;
            };
            let mut record: Vec<&str> = vec![labels[i].as_str(), geom.as_str()];
            for v in &prop_rows[i] {
                record.push(v.as_deref().unwrap_or(""));
            }
            writer.write_record(&record).map_err(|e| e.to_string())?;
            written += 1;
        }
        writer.flush().map_err(|e| e.to_string())?;
    }
    let csv_text = String::from_utf8(buf).map_err(|e| e.to_string())?;

    let renamed: Vec<(String, String)> = columns
        .iter()
        .zip(sanitized.iter())
        .filter(|(o, s)| o != s)
        .map(|(o, s)| (o.clone(), s.clone()))
        .collect();

    serde_json::to_string(&ConvertResult {
        csv: csv_text,
        written,
        skipped,
        renamed,
    })
    .map_err(|e| e.to_string())
}
