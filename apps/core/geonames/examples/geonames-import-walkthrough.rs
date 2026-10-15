//! Manual importer walkthrough against an explicitly supplied disposable,
//! seeded database. No test harness or assertions; prints each outcome.
//!
//! Serves the checked-in `sample/` payloads from a local HTTP fixture with
//! ETags (cities zipped on the fly, as GeoNames ships them) and runs the real
//! importer through: first import, 304 revalidation, local-name preservation
//! across an upstream change, 503 and unreachable fallbacks, `force` refusing
//! fallback, a payload with no valid rows, a database failure in a late batch,
//! the ETag-less cache lifetime and the capability-gated screen entry point.
use axum::{
    extract::{Path as UrlPath, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use bilibi_database::{pool, Error, Result};
use bilibi_geonames::{Dataset, Geonames, ImportOptions, ReferenceImport, IMPORT_SERVICE, SERVICE};
use bilibi_module_registry::Services;
use bilibi_tenancy::Authentication;
use std::{
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
    io::Write,
    path::Path,
    sync::{Arc, Mutex},
};

const SAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/sample");

#[derive(Default)]
struct Fixture {
    files: HashMap<String, Vec<u8>>,
    /// Answer every request with this status instead of a file.
    status: Option<u16>,
    requests: usize,
}
type Shared = Arc<Mutex<Fixture>>;

fn etag(body: &[u8]) -> String {
    let mut hasher = DefaultHasher::new();
    body.hash(&mut hasher);
    format!("\"{:016x}\"", hasher.finish())
}

async fn serve(
    State(fixture): State<Shared>,
    UrlPath(name): UrlPath<String>,
    headers: HeaderMap,
) -> Response {
    let mut fixture = fixture.lock().unwrap();
    fixture.requests += 1;
    if let Some(status) = fixture.status {
        return StatusCode::from_u16(status).unwrap().into_response();
    }
    let Some(body) = fixture.files.get(&name).cloned() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let tag = etag(&body);
    if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        == Some(tag.as_str())
    {
        return (StatusCode::NOT_MODIFIED, [(header::ETAG, tag)]).into_response();
    }
    ([(header::ETAG, tag)], body).into_response()
}

fn zip(entry: &str, body: &[u8]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    writer.start_file(entry, options).unwrap();
    writer.write_all(body).unwrap();
    writer.finish().unwrap().into_inner()
}

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(Path::new(SAMPLE).join(name)).unwrap()
}

fn sample_files() -> HashMap<String, Vec<u8>> {
    HashMap::from([
        ("countryInfo.txt".into(), sample("countryInfo.txt")),
        (
            "admin1CodesASCII.txt".into(),
            sample("admin1CodesASCII.txt"),
        ),
        (
            "cities15000.zip".into(),
            zip("cities15000.txt", &sample("cities15000.txt")),
        ),
    ])
}

async fn run(label: &str, importer: &ReferenceImport, options: &ImportOptions, fixture: &Shared) {
    let before = fixture.lock().unwrap().requests;
    println!("== {label}");
    match importer.run(options).await {
        Ok(reports) => reports.iter().for_each(|report| println!("  {report}")),
        Err(error) => println!("  refused — {error}"),
    }
    println!(
        "  requests made: {}",
        fixture.lock().unwrap().requests - before
    );
}

fn cache(options: &ImportOptions, file: &str) {
    let path = options.cache_dir.join(file);
    let size = std::fs::metadata(&path).map(|m| m.len()).ok();
    let etag = std::fs::read_to_string(path.with_file_name(format!("{file}.etag"))).ok();
    println!("  cache {file}: size {size:?}, etag {etag:?}");
}

async fn counts(db: &bilibi_database::Db) -> Result<()> {
    let row = db.get().await?.query_one(
        "SELECT (SELECT count(*) FROM geonames_countries) c, (SELECT count(*) FROM geonames_admin1) a, (SELECT count(*) FROM geonames_cities) t",
        &[],
    ).await?;
    let (c, a, t): (i64, i64, i64) = (row.get(0), row.get(1), row.get(2));
    println!("  rows: countries {c}, admin1 {a}, cities {t}");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::var("BILIBI_DISPOSABLE_GEONAMES_DEMO").as_deref() != Ok("yes") {
        return Err(Error::Invalid);
    }
    bilibi_base::link();
    bilibi_core::link();
    let db = pool(&std::env::var("DATABASE_URL").map_err(|_| Error::Invalid)?)?;
    let modules = bilibi_module_registry::discover()?;
    let migrations: Vec<_> = modules
        .iter()
        .flat_map(|m| m.migrations.iter().copied())
        .collect();
    bilibi_database::migrate(&db, &migrations).await?;
    let mut services = Services::default();
    services.provide("bootstrap.database", Arc::new(db.clone()))?;
    services.install(&modules)?;
    let importer = services.get::<ReferenceImport>(IMPORT_SERVICE)?;
    let geo = services.get::<Geonames>(SERVICE)?;
    db.get().await?
        .batch_execute("TRUNCATE geonames_cities, geonames_postcodes, geonames_admin1, geonames_countries RESTART IDENTITY")
        .await?;

    let fixture: Shared = Arc::new(Mutex::new(Fixture {
        files: sample_files(),
        ..Fixture::default()
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| Error::Internal)?;
    let port = listener.local_addr().map_err(|_| Error::Internal)?.port();
    let app = Router::new()
        .route("/dump/{name}", get(serve))
        .with_state(fixture.clone());
    tokio::spawn(async move { axum::serve(listener, app).await });

    let cache_dir = std::env::temp_dir().join(format!(
        "bilibi-geonames-walkthrough-{}",
        std::process::id()
    ));
    let options = ImportOptions {
        cache_dir: cache_dir.clone(),
        base_url: format!("http://127.0.0.1:{port}/dump"),
        ..ImportOptions::default()
    };
    println!(
        "fixture http://127.0.0.1:{port}/dump, cache {}",
        cache_dir.display()
    );
    counts(&db).await?;

    run(
        "first import: every dataset downloads",
        &importer,
        &options,
        &fixture,
    )
    .await;
    counts(&db).await?;
    cache(&options, "countryInfo.txt");
    println!(
        "  MY: {:?}",
        geo.get_country("MY")
            .await?
            .map(|c| (c.country, c.capital, c.population))
    );
    println!(
        "  MY Admin1: {:?}",
        geo.list_admin1("MY")
            .await?
            .iter()
            .map(|a| (a.code.clone(), a.name.clone()))
            .collect::<Vec<_>>()
    );
    println!(
        "  MY cities 'K': {:?}",
        geo.search_city_names("MY", "K", None).await?
    );
    println!(
        "  Osaka (impossible date): {:?}",
        geo.get_city_by_geoname_id(1853909)
            .await?
            .map(|c| c.modification_date)
    );

    run(
        "second run: the server confirms every cached file (304)",
        &importer,
        &options,
        &fixture,
    )
    .await;

    println!("== an operator renames MY and MY.14 locally");
    db.get()
        .await?
        .batch_execute(
            "UPDATE geonames_countries SET country='Malaysia (local)' WHERE iso='MY';
         UPDATE geonames_admin1 SET name='Wilayah Persekutuan Kuala Lumpur' WHERE code='MY.14';",
        )
        .await?;
    {
        let mut f = fixture.lock().unwrap();
        let countries = String::from_utf8(sample("countryInfo.txt"))
            .unwrap()
            .replace(
                "Malaysia\tKuala Lumpur\t329750\t31528585",
                "Malaysia (upstream)\tPutrajaya\t329750\t33000000",
            );
        f.files
            .insert("countryInfo.txt".into(), countries.into_bytes());
        let admin1 = String::from_utf8(sample("admin1CodesASCII.txt"))
            .unwrap()
            .replace(
                "MY.14\tKuala Lumpur\tKuala Lumpur",
                "MY.14\tKL (upstream)\tKL upstream ASCII",
            );
        f.files
            .insert("admin1CodesASCII.txt".into(), admin1.into_bytes());
    }
    run(
        "upstream changed MY and MY.14: a re-import refreshes facts, keeps local names",
        &importer,
        &options,
        &fixture,
    )
    .await;
    println!(
        "  MY: {:?}",
        geo.get_country("MY")
            .await?
            .map(|c| (c.country, c.capital, c.population))
    );
    println!(
        "  MY.14: {:?}",
        geo.get_admin1("MY.14").await?.map(|a| (a.name, a.alt_name))
    );

    fixture.lock().unwrap().status = Some(503);
    run(
        "the server answers 503: imports continue from the cache",
        &importer,
        &options,
        &fixture,
    )
    .await;
    run(
        "…but --force refuses to fall back",
        &importer,
        &ImportOptions {
            force: true,
            ..options.clone()
        },
        &fixture,
    )
    .await;
    fixture.lock().unwrap().status = None;

    let unreachable = ImportOptions {
        base_url: "http://127.0.0.1:9/dump".into(),
        datasets: vec![Dataset::Countries],
        ..options.clone()
    };
    run(
        "the server is unreachable: countries continue from the cache",
        &importer,
        &unreachable,
        &fixture,
    )
    .await;

    println!("== upstream countries file is now only comments");
    cache(&options, "countryInfo.txt");
    fixture.lock().unwrap().files.insert(
        "countryInfo.txt".into(),
        b"# maintenance\n# nothing here\n".to_vec(),
    );
    run(
        "a payload with no valid rows is refused and the cache restored",
        &importer,
        &ImportOptions {
            datasets: vec![Dataset::Countries],
            ..options.clone()
        },
        &fixture,
    )
    .await;
    cache(&options, "countryInfo.txt");
    counts(&db).await?;

    println!("== upstream cities: 1,200 valid rows, then one naming an unknown country (row 1,201, third batch)");
    let mut big = String::new();
    for n in 0..1200 {
        big.push_str(&format!("{}\tTown {n}\tTown {n}\t\t3.{n}\t101.{n}\tP\tPPL\tMY\t\t14\t\t\t\t{n}\t\t1\tAsia/Kuala_Lumpur\t2024-01-01\n", 5_000_000 + n));
    }
    big.push_str(
        "5999999\tNowhere\tNowhere\t\t1.0\t1.0\tP\tPPL\tZZ\t\t\t\t\t\t1\t\t1\tUTC\t2024-01-01\n",
    );
    fixture.lock().unwrap().files.insert(
        "cities15000.zip".into(),
        zip("cities15000.txt", big.as_bytes()),
    );
    cache(&options, "cities15000.zip");
    run(
        "the database refuses one row: the whole dataset rolls back",
        &importer,
        &ImportOptions {
            datasets: vec![Dataset::Cities],
            ..options.clone()
        },
        &fixture,
    )
    .await;
    counts(&db).await?;
    cache(&options, "cities15000.zip");
    println!(
        "  extracted entry after failure exists: {}",
        options.cache_dir.join("cities15000.txt").exists()
    );

    println!("== the server is restored to the sample payloads");
    fixture.lock().unwrap().files = sample_files();
    run(
        "the restored cache is revalidated (304) and re-extracted",
        &importer,
        &ImportOptions {
            datasets: vec![Dataset::Cities],
            ..options.clone()
        },
        &fixture,
    )
    .await;

    std::fs::remove_file(options.cache_dir.join("admin1CodesASCII.txt.etag"))
        .map_err(|_| Error::Internal)?;
    run(
        "an Admin1 cache without an ETag is trusted for 7 days: no request",
        &importer,
        &ImportOptions {
            datasets: vec![Dataset::Admin1],
            ..options.clone()
        },
        &fixture,
    )
    .await;
    run(
        "…unless forced",
        &importer,
        &ImportOptions {
            datasets: vec![Dataset::Admin1],
            force: true,
            ..options.clone()
        },
        &fixture,
    )
    .await;

    println!("== the screen entry point checks admin.geonames.update (platform) on every call");
    let authentication = services.get::<Authentication>("base/tenancy.authentication")?;
    let client = db.get().await?;
    client.execute("INSERT INTO platform_operator(singleton,tenant_id) VALUES(true,1) ON CONFLICT DO NOTHING", &[]).await?;
    let admin = authentication
        .authenticate(&**client, 1, 4, "walkthrough".into())
        .await?;
    let viewer = authentication
        .authenticate(&**client, 1, 2, "walkthrough".into())
        .await?;
    let countries = ImportOptions {
        datasets: vec![Dataset::Countries],
        ..options.clone()
    };
    for (label, scope) in [("operator admin", &admin), ("viewer", &viewer)] {
        match importer.run_as(scope, &countries).await {
            Ok(reports) => reports
                .iter()
                .for_each(|report| println!("  {label}: {report}")),
            Err(error) => println!("  {label}: refused — {error}"),
        }
    }

    let exchanges: i64 = client
        .query_one("SELECT count(*) FROM base_integration_exchanges WHERE system='geonames' AND tenant_id IS NULL", &[])
        .await?
        .get(0);
    println!("== {exchanges} GeoNames downloads recorded as system exchanges");
    let _ = std::fs::remove_dir_all(&cache_dir);
    Ok(())
}
