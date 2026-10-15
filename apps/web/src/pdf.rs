//! Single-use signed PDF render route: the one unauthenticated page that serves
//! owner HTML. The token is the only credential and names a `render_view` call
//! already waiting in this process; Artifacts reauthorizes the live actor
//! before the template runs. Failures are empty and say nothing about why.
use crate::App;
use axum::{
    extract::{Path, State},
    http::{HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use bilibi_base::artifacts::{Artifacts, Served, RENDER_HEADERS};

pub use bilibi_base::artifacts::RENDER_ROUTE as RENDER;

pub async fn render(State(app): State<App>, Path(token): Path<String>) -> Response {
    let served = match app.services.get::<Artifacts>("base/artifacts.store") {
        Ok(artifacts) => artifacts.serve_render(&token).await,
        Err(_) => Served::NotFound,
    };
    let mut response = match served {
        Served::Page(html) => (StatusCode::OK, html).into_response(),
        Served::NotFound => StatusCode::NOT_FOUND.into_response(),
        Served::Failed => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let headers = response.headers_mut();
    for (name, value) in RENDER_HEADERS {
        headers.insert(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    }
    response
}
