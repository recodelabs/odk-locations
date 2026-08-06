# Generates the parquet fixtures. Run from the repo root:
#   uv run --with geopandas --with pyarrow python tests/fixtures/make_fixtures.py
# Artifacts are committed; re-run only when fixtures must change.
import geopandas as gpd
import pandas as pd
from shapely.geometry import Point

FIX = "tests/fixtures"

plain = pd.DataFrame(
    {"name": ["P1", "P2"], "lat": [6.5, 6.6], "lon": [3.4, 3.5], "pop": [10, 20]}
)
plain.to_parquet(f"{FIX}/points_plain.parquet", index=False)

gdf = gpd.GeoDataFrame(
    {"name": ["G1", None], "pop": [1, 2]},
    geometry=[Point(3.4, 6.5), None],
    crs="EPSG:4326",
)
gdf.to_parquet(f"{FIX}/points_geo.parquet", index=False)

gdf3857 = gpd.GeoDataFrame(
    {"name": ["M1"]},
    geometry=[Point(111319.49079327357, 111325.14286638486)],  # ~ (1.0, 1.0) deg
    crs="EPSG:3857",
)
gdf3857.to_parquet(f"{FIX}/points_3857.parquet", index=False)
print("fixtures written")
