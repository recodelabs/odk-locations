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
    let mut writer =
        csv::Writer::from_path(path).with_context(|| format!("writing {}", path.display()))?;
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
    Ok(Summary {
        written,
        skipped,
        renamed,
    })
}
