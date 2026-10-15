# GeoNames sample payloads

A few rows in the exact layouts of GeoNames' `countryInfo.txt`,
`admin1CodesASCII.txt` and `cities15000.txt` (the importer reads the last from
`cities15000.zip`). They drive the importer's runtime proof without the network:
`examples/geonames-import-walkthrough.rs` serves them over a local HTTP
fixture with ETags, and the `geonames-import --base-url` command can read them
from any static server.

Each file also carries lines the importer must skip, so a run reports skipped
rows: comments, a truncated line, an invalid ISO code (`X1`), an out-of-range
latitude and an Admin1 line with missing columns. Osaka's
modification date (`2023-02-30`) is not a real date and imports as unknown.

Expected import: countries 6 imported / 4 skipped, Admin1 10 / 1, cities
10 / 2.

Values are illustrative trims of GeoNames data (<https://www.geonames.org/>,
licensed CC BY 4.0); do not use them as reference data.
