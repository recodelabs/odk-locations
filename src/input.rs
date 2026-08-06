//! Per-format readers producing a uniform Dataset: geometries plus
//! stringified attribute columns in input order.

use anyhow::{anyhow, bail, Context, Result};
use arrow::array::{Array, BinaryArray, LargeBinaryArray};
use arrow::record_batch::RecordBatch;
use geo::Geometry;
use geozero::{wkb::Wkb, ToGeo};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
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
        "parquet" => read_parquet(path, lat_col, lng_col),
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
    use std::cell::Cell;
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

    let failed = Cell::new(false);
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
                    failed.set(true);
                    c
                }
            }
        });
    }
    if failed.get() {
        bail!("coordinate transform from EPSG:{code} failed for at least one point");
    }
    Ok(())
}
