//! Upload inspection. The MIME type comes from libmagic (the `file` command)
//! reading the bytes the server holds, never from a client filename or claimed
//! content type. The command gets an argument list and stdin: no shell, no
//! caller-selected executable, no temporary file.
use crate::{Error, Result};
use std::{process::Stdio, time::Duration};
use tokio::{io::AsyncWriteExt, process::Command};

/// Markup a browser would execute if served inline. Always refused.
const ACTIVE_TYPES: [&str; 8] = [
    "image/svg+xml",
    "text/html",
    "application/xhtml+xml",
    "application/xml",
    "text/xml",
    "text/javascript",
    "application/javascript",
    "application/x-javascript",
];

pub struct Upload {
    pub filename: String,
    pub bytes: Vec<u8>,
}
/// A subject owner may narrow, never widen, the operator's limits.
#[derive(Clone, Debug, Default)]
pub struct Rules {
    pub max_bytes: Option<u64>,
    pub allowed_mime_types: Option<Vec<String>>,
}
pub struct Inspected {
    pub filename: String,
    pub mime_type: String,
    pub size: i64,
}

pub struct Limits<'a> {
    pub max_bytes: u64,
    pub attachment_types: &'a [String],
}

pub fn is_active(mime: &str) -> bool {
    ACTIVE_TYPES.contains(&mime)
}
/// Active content is refused everywhere; attachments also need the allowlist.
pub fn allowed(mime: &str, attachment: bool, types: &[String]) -> Result<()> {
    if is_active(mime) || (attachment && !types.iter().any(|allowed| allowed == mime)) {
        return Err(Error::DisallowedType);
    }
    Ok(())
}

pub async fn inspect(
    upload: &Upload,
    attachment: bool,
    rules: &Rules,
    limits: Limits<'_>,
) -> Result<Inspected> {
    let maximum = rules
        .max_bytes
        .map_or(limits.max_bytes, |own| own.min(limits.max_bytes));
    if rules.max_bytes == Some(0) {
        return Err(Error::InvalidRules);
    }
    let size = upload.bytes.len() as u64;
    if size == 0 || size > maximum {
        return Err(Error::InvalidSize);
    }
    let mime = detect(&upload.bytes).await?;
    allowed(&mime, attachment, limits.attachment_types)?;
    if rules
        .allowed_mime_types
        .as_ref()
        .is_some_and(|types| !types.contains(&mime))
    {
        return Err(Error::DisallowedType);
    }
    Ok(Inspected {
        filename: clean_filename(&upload.filename),
        mime_type: mime,
        size: size as i64,
    })
}

/// Browsers can send a path; keep the last segment, drop control characters,
/// and fit the 255-character column.
pub fn clean_filename(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = base.chars().filter(|c| !c.is_control()).take(255).collect();
    let cleaned = cleaned.trim().to_string();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        "upload".into()
    } else {
        cleaned
    }
}

/// libmagic's verdict, normalized to a bare lowercase `type/subtype`.
pub async fn detect(bytes: &[u8]) -> Result<String> {
    let mut child = Command::new("file")
        .args(["--brief", "--mime-type", "--", "-"])
        .env("LC_ALL", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => Error::MimeDetectionUnavailable,
            _ => Error::InvalidUpload,
        })?;
    let mut stdin = child.stdin.take().ok_or(Error::InvalidUpload)?;
    // `file` reads a prefix and may exit first; a closed pipe is not a failure.
    let feed = async move {
        let _ = stdin.write_all(bytes).await;
        drop(stdin);
    };
    let (_, output) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(feed, child.wait_with_output())
    })
    .await
    .map_err(|_| Error::InvalidUpload)?;
    let output = output.map_err(|_| Error::InvalidUpload)?;
    if !output.status.success() {
        return Err(Error::InvalidUpload);
    }
    let mime = String::from_utf8_lossy(&output.stdout)
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let valid = mime.len() <= 255
        && mime.split_once('/').is_some_and(|(kind, subtype)| {
            !kind.is_empty()
                && !subtype.is_empty()
                && mime
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"/+-._".contains(&b))
        });
    if valid {
        Ok(mime)
    } else {
        Err(Error::InvalidUpload)
    }
}
