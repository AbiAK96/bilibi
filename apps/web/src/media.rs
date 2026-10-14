//! Signed media stream: the one unauthenticated route that serves stored bytes.
//! The token is the only credential; Media rechecks tenant, private-binding and
//! storage on every request and the response is never cacheable or sniffable.
use crate::App;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use bilibi_base::media::{Error, Media};
use std::collections::HashMap;

pub const STREAM: &str = "/media/assets/{id}/stream";

pub async fn stream(
    State(app): State<App>,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let Some(id) = id
        .bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| id.parse::<i64>().ok())
        .flatten()
        .filter(|id| *id > 0)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let result = async {
        let media = app
            .services
            .get::<Media>("base/media.store")
            .map_err(|_| Error::StorageUnavailable)?;
        let client = app.db.get().await.map_err(bilibi_database::Error::from)?;
        media
            .read_signed_asset(&**client, id, query.get("token").map_or("", String::as_str))
            .await
    }
    .await;
    match result {
        Ok(file) => {
            let delivery = file.delivery(false);
            let mut response = (StatusCode::OK, file.bytes).into_response();
            let headers = response.headers_mut();
            if let Ok(value) = HeaderValue::from_str(&delivery.content_type) {
                headers.insert(axum::http::header::CONTENT_TYPE, value);
            }
            for (name, value) in delivery.headers {
                if let Ok(value) = HeaderValue::from_str(&value) {
                    headers.insert(HeaderName::from_static(name), value);
                }
            }
            response
        }
        Err(Error::InvalidToken | Error::ExpiredToken | Error::PrivateAsset) => {
            (StatusCode::FORBIDDEN, "Forbidden").into_response()
        }
        Err(Error::NotFound | Error::SourceNotStored | Error::StorageUnavailable) => {
            (StatusCode::NOT_FOUND, "Not found").into_response()
        }
        Err(_) => (StatusCode::SERVICE_UNAVAILABLE, "Unavailable").into_response(),
    }
}
