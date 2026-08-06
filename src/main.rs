use anyhow::{bail, Result};
use clap::Parser;
use odk_locations::convert::{
    detect_family, geopoint, geoshape, make_labels, sanitize_columns, Family, DEFAULT_MAX_VERTICES,
};
use odk_locations::input::read_input;
use odk_locations::output::write_entities;
use std::path::PathBuf;

const NAME_LIKE: [&str; 3] = ["name", "title", "label"];

/// Convert GeoJSON / (Geo)Parquet / CSV point-or-polygon data into an ODK
/// entity CSV (label, geometry in ODK geopoint/geoshape format, all
/// attributes as properties). Ready for ODK Central's bulk entity upload.
#[derive(Parser)]
#[command(name = "odk-locations", version)]
struct Cli {
    /// Input file (.geojson/.json, .csv, .parquet)
    input: PathBuf,
    /// Output CSV path [default: <input-stem>-entities.csv]
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Attribute column used for entity labels [default: name/title/label,
    /// else feature-<n>]
    #[arg(long)]
    label_column: Option<String>,
    /// Polygon geometry mode (points always export as geopoints)
    #[arg(long, value_parser = ["centroid", "boundary"], default_value = "boundary")]
    geometry: String,
    /// Geoshape ring vertex cap
    #[arg(long, default_value_t = DEFAULT_MAX_VERTICES)]
    max_vertices: usize,
    /// Latitude column (CSV/plain-parquet inputs without geometry)
    #[arg(long)]
    lat_column: Option<String>,
    /// Longitude column (CSV/plain-parquet inputs without geometry)
    #[arg(long)]
    lng_column: Option<String>,
    /// Suppress the summary printed to stderr
    #[arg(long)]
    quiet: bool,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let dataset = read_input(
        &cli.input,
        cli.lat_column.as_deref(),
        cli.lng_column.as_deref(),
    )?;
    let family = detect_family(&dataset.geoms).map_err(anyhow::Error::msg)?;

    // Label column: explicit flag (must exist), else first name-like column.
    let label_idx: Option<usize> =
        match &cli.label_column {
            Some(name) => Some(dataset.columns.iter().position(|c| c == name).ok_or_else(
                || {
                    anyhow::anyhow!(
                        "label column {name:?} not found (columns: {:?})",
                        dataset.columns
                    )
                },
            )?),
            None => dataset
                .columns
                .iter()
                .position(|c| NAME_LIKE.contains(&c.to_ascii_lowercase().as_str())),
        };
    let label_values: Option<Vec<Option<String>>> =
        label_idx.map(|i| dataset.rows.iter().map(|r| r[i].clone()).collect());
    let labels = make_labels(label_values.as_deref(), dataset.geoms.len());

    // The label column is consumed — drop it from the property columns.
    let (prop_columns, prop_rows): (Vec<String>, Vec<Vec<Option<String>>>) = match label_idx {
        Some(li) => (
            dataset
                .columns
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != li)
                .map(|(_, c)| c.clone())
                .collect(),
            dataset
                .rows
                .iter()
                .map(|r| {
                    r.iter()
                        .enumerate()
                        .filter(|(i, _)| *i != li)
                        .map(|(_, v)| v.clone())
                        .collect()
                })
                .collect(),
        ),
        None => (dataset.columns.clone(), dataset.rows.clone()),
    };
    let sanitized = sanitize_columns(&prop_columns);

    let geometries: Vec<Option<String>> = dataset
        .geoms
        .iter()
        .map(|g| {
            g.as_ref()
                .and_then(|g| match (family, cli.geometry.as_str()) {
                    (Family::Polygon, "boundary") => geoshape(g, cli.max_vertices),
                    _ => geopoint(g),
                })
        })
        .collect();

    if geometries.iter().all(Option::is_none) {
        bail!("no exportable features: every row has a missing or empty geometry");
    }

    let output = cli.output.clone().unwrap_or_else(|| {
        let stem = cli
            .input
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("output");
        cli.input.with_file_name(format!("{stem}-entities.csv"))
    });
    let summary = write_entities(
        &output,
        &labels,
        &geometries,
        &sanitized,
        &prop_columns,
        &prop_rows,
    )?;

    if !cli.quiet {
        eprintln!(
            "{} written, {} skipped (empty geometry) -> {}",
            summary.written,
            summary.skipped,
            output.display()
        );
        for (old, new) in &summary.renamed {
            eprintln!("renamed column {old:?} -> {new:?}");
        }
    }
    Ok(())
}
