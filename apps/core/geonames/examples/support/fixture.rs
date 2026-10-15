//! A local GeoNames stand-in for the importer walkthroughs: serves the
//! checked-in `sample/` payloads with ETags (archives zipped on the fly, as
//! GeoNames ships them) under `/dump/{name}` and `/zip/{name}`, and can be
//! told to answer every request with one status instead. Not an example
//! itself; each walkthrough includes it with `#[path]`.
use axum::{
    extract::{Path as UrlPath, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use std::{
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
    io::Write,
    path::Path,
    sync::{Arc, Mutex},
};

const SAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/sample");

#[derive(Default)]
pub struct Fixture {
    pub files: HashMap<String, Vec<u8>>,
    /// Answer every request with this status instead of a file.
    pub status: Option<u16>,
    pub requests: usize,
}
pub type Shared = Arc<Mutex<Fixture>>;

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

pub fn zip(entry: &str, body: &[u8]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    writer.start_file(entry, options).unwrap();
    writer.write_all(body).unwrap();
    writer.finish().unwrap().into_inner()
}

pub fn sample(name: &str) -> Vec<u8> {
    std::fs::read(Path::new(SAMPLE).join(name)).unwrap()
}

/// The three default datasets plus Malaysia's postcodes, as GeoNames serves
/// them.
pub fn sample_files() -> HashMap<String, Vec<u8>> {
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
        ("MY.zip".into(), zip("MY.txt", &sample("MY.txt"))),
    ])
}

/// Starts the fixture on a free local port; returns it and the shared state.
pub async fn start() -> std::io::Result<(u16, Shared)> {
    let fixture: Shared = Arc::new(Mutex::new(Fixture {
        files: sample_files(),
        ..Fixture::default()
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let app = Router::new()
        .route("/dump/{name}", get(serve))
        .route("/zip/{name}", get(serve))
        .with_state(fixture.clone());
    tokio::spawn(async move { axum::serve(listener, app).await });
    Ok((port, fixture))
}

/// A process-private cache directory under the system temporary directory.
pub fn cache_dir(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("bilibi-geonames-{label}-{}", std::process::id()))
}

/// What the cache holds for one file.
pub fn describe_cache(dir: &Path, file: &str) {
    let path = dir.join(file);
    let size = std::fs::metadata(&path).map(|m| m.len()).ok();
    let etag = std::fs::read_to_string(path.with_file_name(format!("{file}.etag"))).ok();
    println!("  cache {file}: size {size:?}, etag {etag:?}");
}
