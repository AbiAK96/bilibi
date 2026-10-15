//! Download-and-import of the GeoNames reference datasets (Bilimbi
//! `Core.Geonames.ReferenceData` and the `bilimbi.geonames.import` task).
//!
//! Datasets run in dependency order (countries, Admin1, cities), each atomic on
//! its own; the first failure stops the run and earlier datasets stay
//! committed. Before a dataset downloads, its cached payload and ETag are
//! copied aside. If the download, extraction or import then fails, the
//! known-good cache is restored and any extracted entry deleted, so the next
//! run neither trusts a payload that failed to import nor revalidates it.
use crate::{
    download::{self, DownloadFailure, DownloadStatus, Downloaded, Request},
    importer::{self, ImportFailure},
    UPDATE,
};
use bilibi_database::Db;
use bilibi_integration::Integration;
use bilibi_tenancy::Scope;
use std::{
    fmt,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

/// The registry service id of the importer.
pub const IMPORT_SERVICE: &str = "core/geonames.import";
/// GeoNames' dump directory. A mirror with the same layout may replace it.
pub const DEFAULT_BASE_URL: &str = "https://download.geonames.org/export/dump";
/// An extracted entry larger than this is refused (the real cities file is
/// a few MiB).
const MAX_EXTRACTED_BYTES: u64 = 2 << 30;

pub const USAGE: &str = "usage: bilibi-web geonames-import [--datasets countries,admin1,cities] [--cache-dir DIR] [--base-url URL] [--force]";

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Dataset {
    Countries,
    Admin1,
    Cities,
}
impl Dataset {
    /// Import order: Admin1 and cities name their country.
    pub const ALL: [Dataset; 3] = [Self::Countries, Self::Admin1, Self::Cities];
    pub fn key(self) -> &'static str {
        match self {
            Self::Countries => "countries",
            Self::Admin1 => "admin1",
            Self::Cities => "cities",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.key() == value.trim())
    }
    /// The upstream file name and, for an archive, the entry to import.
    fn file(self) -> (&'static str, Option<&'static str>) {
        match self {
            Self::Countries => ("countryInfo.txt", None),
            Self::Admin1 => ("admin1CodesASCII.txt", None),
            Self::Cities => ("cities15000.zip", Some("cities15000.txt")),
        }
    }
}
impl fmt::Display for Dataset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

#[derive(Clone, Debug)]
pub struct ImportOptions {
    pub datasets: Vec<Dataset>,
    /// Durable download cache; see `default_cache_dir`.
    pub cache_dir: PathBuf,
    pub base_url: String,
    /// Re-download ignoring the cache, and never fall back to it.
    pub force: bool,
    /// How long an ETag-less cached file is trusted without a request.
    pub ttl_days: u64,
    /// The whole-download timeout.
    pub timeout: Duration,
}
impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            datasets: Dataset::ALL.to_vec(),
            cache_dir: default_cache_dir(),
            base_url: DEFAULT_BASE_URL.into(),
            force: false,
            ttl_days: 7,
            timeout: Duration::from_secs(300),
        }
    }
}
impl ImportOptions {
    /// Parses the `geonames-import` command's arguments.
    pub fn from_args(args: &[String]) -> Result<Self, String> {
        let mut options = Self::default();
        let mut args = args.iter();
        while let Some(flag) = args.next() {
            let mut value = || {
                args.next()
                    .filter(|value| !value.starts_with("--"))
                    .ok_or(format!("{flag} needs a value"))
            };
            match flag.as_str() {
                "--datasets" => {
                    let mut datasets = Vec::new();
                    for name in value()?.split(',').filter(|name| !name.trim().is_empty()) {
                        let dataset = Dataset::parse(name)
                            .ok_or(ImportError::InvalidDataset(name.into()).to_string())?;
                        if !datasets.contains(&dataset) {
                            datasets.push(dataset);
                        }
                    }
                    options.datasets = datasets;
                }
                "--cache-dir" => options.cache_dir = PathBuf::from(value()?),
                "--base-url" => options.base_url = value()?.trim_end_matches('/').to_string(),
                "--force" => options.force = true,
                other => return Err(format!("unexpected argument {other}")),
            }
        }
        Ok(options)
    }
}

/// `GEONAMES_CACHE_DIR`, else `bilibi/geonames` under the system temporary
/// directory. Use a durable directory in production so ETags survive.
pub fn default_cache_dir() -> PathBuf {
    std::env::var_os("GEONAMES_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("bilibi").join("geonames"))
}

#[derive(Clone, Debug)]
pub struct DatasetReport {
    pub dataset: Dataset,
    pub imported: u64,
    pub skipped: u64,
    pub download: DownloadStatus,
    /// The cache file's age when the import read a cached payload.
    pub cached_at: Option<SystemTime>,
}
impl DatasetReport {
    pub fn cached(&self) -> bool {
        !matches!(self.download, DownloadStatus::Fetched(_))
    }
}
impl fmt::Display for DatasetReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let source = match &self.download {
            DownloadStatus::Fetched(_) => "fresh download".to_string(),
            DownloadStatus::NotModified => "cached download; server confirmed current".into(),
            DownloadStatus::Fresh => "cached download; within the cache lifetime".into(),
            DownloadStatus::Fallback(cause) => {
                let age = self
                    .cached_at
                    .and_then(|at| SystemTime::now().duration_since(at).ok())
                    .map_or("unknown age".into(), |age| {
                        format!("{} min old", age.as_secs() / 60)
                    });
                format!("cached download kept after a failed download ({cause:?}); cache {age}")
            }
        };
        write!(
            f,
            "{}: imported {}, skipped {} ({source})",
            self.dataset, self.imported, self.skipped
        )
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("unknown dataset {0:?}; use countries, admin1 or cities")]
    InvalidDataset(String),
    #[error("{0} download failed: {1}")]
    Download(Dataset, DownloadFailure),
    #[error("{0} extraction failed: {1}")]
    Extract(Dataset, String),
    #[error("{0} import failed: {1}")]
    Import(Dataset, ImportFailure),
    #[error(transparent)]
    Access(#[from] bilibi_database::Error),
}

pub struct ReferenceImport {
    db: Db,
    integration: Arc<Integration>,
}
impl ReferenceImport {
    pub fn new(db: Db, integration: Arc<Integration>) -> Self {
        Self { db, integration }
    }

    /// The operator command's import: no signed-in actor. Downloads are
    /// recorded as system exchanges (installation-wide data, no tenant).
    pub async fn run(&self, options: &ImportOptions) -> Result<Vec<DatasetReport>, ImportError> {
        let mut reports = Vec::new();
        for dataset in Dataset::ALL {
            if options.datasets.contains(&dataset) {
                reports.push(self.download_and_import(dataset, options).await?);
            }
        }
        Ok(reports)
    }

    /// A screen's import: the actor must hold `admin.geonames.update` (a
    /// platform capability) at the moment of the call.
    pub async fn run_as(
        &self,
        scope: &Scope,
        options: &ImportOptions,
    ) -> Result<Vec<DatasetReport>, ImportError> {
        let client = self.db.get().await.map_err(bilibi_database::Error::from)?;
        bilibi_authz::authorize_key(&**client, scope, UPDATE).await?;
        drop(client);
        self.run(options).await
    }

    async fn download_and_import(
        &self,
        dataset: Dataset,
        options: &ImportOptions,
    ) -> Result<DatasetReport, ImportError> {
        let (filename, entry) = dataset.file();
        let destination = options.cache_dir.join(filename);
        let extracted = entry.map(|entry| options.cache_dir.join(entry));
        let backup = Backup::take(&destination)
            .await
            .map_err(|error| ImportError::Download(dataset, error))?;
        let result = self
            .attempt(dataset, options, &destination, extracted.as_deref())
            .await;
        match &result {
            Ok(_) => backup.discard().await,
            Err(_) => {
                backup.restore().await;
                if let Some(extracted) = &extracted {
                    let _ = tokio::fs::remove_file(extracted).await;
                }
            }
        }
        result
    }

    async fn attempt(
        &self,
        dataset: Dataset,
        options: &ImportOptions,
        destination: &Path,
        extracted: Option<&Path>,
    ) -> Result<DatasetReport, ImportError> {
        let url = format!(
            "{}/{}",
            options.base_url.trim_end_matches('/'),
            dataset.file().0
        );
        let downloaded = download::download(
            &self.integration,
            Request {
                url: &url,
                destination,
                force: options.force,
                ttl_days: options.ttl_days,
                timeout: options.timeout,
            },
        )
        .await
        .map_err(|error| ImportError::Download(dataset, error))?;
        let path = match (extracted, dataset.file().1) {
            (Some(extracted), Some(entry)) => {
                extract(destination, entry, extracted, &downloaded)
                    .await
                    .map_err(|error| ImportError::Extract(dataset, error))?;
                extracted.to_path_buf()
            }
            _ => destination.to_path_buf(),
        };
        let counts = match dataset {
            Dataset::Countries => importer::countries(&self.db, &path).await,
            Dataset::Admin1 => importer::admin1(&self.db, &path).await,
            Dataset::Cities => importer::cities(&self.db, &path).await,
        }
        .map_err(|error| ImportError::Import(dataset, error))?;
        Ok(DatasetReport {
            dataset,
            imported: counts.imported,
            skipped: counts.skipped,
            download: downloaded.status,
            cached_at: downloaded.cached_at,
        })
    }
}

/// Reuses an extraction only when the archive itself came from cache; a new
/// archive is always extracted afresh. The entry is copied out in a stream,
/// written beside its target and renamed into place.
async fn extract(
    archive: &Path,
    entry: &'static str,
    target: &Path,
    downloaded: &Downloaded,
) -> Result<(), String> {
    if downloaded.cached() && target.is_file() {
        return Ok(());
    }
    let _ = tokio::fs::remove_file(target).await;
    let (archive, target) = (archive.to_path_buf(), target.to_path_buf());
    tokio::task::spawn_blocking(move || {
        let file = std::fs::File::open(&archive).map_err(|error| error.to_string())?;
        let mut zip = zip::ZipArchive::new(file).map_err(|error| error.to_string())?;
        let source = zip.by_name(entry).map_err(|error| error.to_string())?;
        let partial = download::sibling(&target, &format!(".extract-{}", download::unique()));
        let copied = (|| {
            let mut output = std::fs::File::create(&partial)?;
            let copied = std::io::copy(&mut source.take(MAX_EXTRACTED_BYTES + 1), &mut output)?;
            output.flush()?;
            Ok::<_, std::io::Error>(copied)
        })();
        let outcome = match copied {
            Err(error) => Err(error.to_string()),
            Ok(copied) if copied > MAX_EXTRACTED_BYTES => Err(format!("{entry} is too large")),
            Ok(_) => std::fs::rename(&partial, &target).map_err(|error| error.to_string()),
        };
        if outcome.is_err() {
            let _ = std::fs::remove_file(&partial);
        }
        outcome
    })
    .await
    .map_err(|_| "extraction task failed".to_string())?
}

/// A copy of the cached payload and its ETag, taken before a download.
struct Backup {
    pairs: [(PathBuf, PathBuf); 2],
}
impl Backup {
    async fn take(destination: &Path) -> Result<Self, DownloadFailure> {
        let backup = download::sibling(destination, &format!(".good-{}", download::unique()));
        let pairs = [
            (destination.to_path_buf(), backup.clone()),
            (
                download::etag_path(destination),
                download::etag_path(&backup),
            ),
        ];
        if let Some(parent) = destination.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| DownloadFailure::Io(error.to_string()))?;
        }
        for (path, copy) in &pairs {
            if path.is_file() {
                tokio::fs::copy(path, copy)
                    .await
                    .map_err(|error| DownloadFailure::Io(error.to_string()))?;
            }
        }
        Ok(Self { pairs })
    }
    /// Puts the known-good files back; a file that had no copy is removed.
    async fn restore(self) {
        for (path, copy) in &self.pairs {
            if copy.is_file() {
                let _ = tokio::fs::rename(copy, path).await;
            } else {
                let _ = tokio::fs::remove_file(path).await;
            }
        }
    }
    async fn discard(self) {
        for (_, copy) in &self.pairs {
            let _ = tokio::fs::remove_file(copy).await;
        }
    }
}
