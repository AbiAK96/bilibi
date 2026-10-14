//! How stored bytes leave the server. Only raster images render inline; every
//! other type, and every private file, downloads as an opaque octet stream
//! under a sandboxing policy. The filename is percent-encoded (RFC 5987) so it
//! cannot inject headers.
const INLINE: [&str; 4] = ["image/jpeg", "image/png", "image/gif", "image/webp"];

pub struct StreamFile {
    pub bytes: Vec<u8>,
    pub filename: String,
    pub mime_type: Option<String>,
}
pub struct Delivery {
    pub content_type: String,
    pub headers: Vec<(&'static str, String)>,
}
impl StreamFile {
    pub fn delivery(&self, private: bool) -> Delivery {
        let mime = self
            .mime_type
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        let inline = !private && INLINE.contains(&mime.as_str());
        let filename: String = self
            .filename
            .bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                    (b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect();
        Delivery {
            content_type: if inline {
                mime
            } else {
                "application/octet-stream".into()
            },
            headers: vec![
                (
                    "content-disposition",
                    format!(
                        "{}; filename*=UTF-8''{filename}",
                        if inline { "inline" } else { "attachment" }
                    ),
                ),
                ("x-content-type-options", "nosniff".into()),
                (
                    "content-security-policy",
                    "default-src 'none'; sandbox".into(),
                ),
                ("cache-control", "private, no-store".into()),
            ],
        }
    }
}
