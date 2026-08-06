//! Per-format readers producing a uniform Dataset: geometries plus
//! stringified attribute columns in input order.

use anyhow::{anyhow, bail, Context, Result};
use geo::Geometry;
use std::path::Path;

#[derive(Debug)]
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
