# GeoNames sample payloads

A few rows in the exact layouts of GeoNames' `countryInfo.txt`,
`admin1CodesASCII.txt`, `cities15000.txt` (the importer reads it from
`cities15000.zip`) and the postal-code `MY.txt` (read from `export/zip/MY.zip`).
They drive the importer's runtime proof without the network: the walkthroughs
under `examples/` serve them over a local HTTP fixture with ETags, and the
`geonames-import --base-url`/`--postcodes-url` command can read them from any
static server.

Each file also carries lines the importer must skip, so a run reports skipped
rows: comments, a truncated line, an invalid ISO code (`X1`), an out-of-range
latitude, an Admin1 line with missing columns, and in `MY.txt` a line for
another country, an out-of-range latitude and an accuracy outside 1–6. Osaka's
modification date (`2023-02-30`) is not a real date and imports as unknown.

Expected import: countries 6 imported / 4 skipped, Admin1 10 / 1, cities
10 / 2, postcodes MY 10 / 5.

Values are illustrative trims of GeoNames data (<https://www.geonames.org/>,
licensed CC BY 4.0); do not use them as reference data.
