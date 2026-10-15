//! Axum adapter for Perf. Perf owns what is recorded; this only says which
//! route a request matched and how it ended. The recorder never awaits, so a
//! slow or failing history store cannot slow a response.
use axum::{
    body::HttpBody,
    extract::{MatchedPath, Request, State},
    middleware::Next,
    response::Response,
};
use bilibi_base::perf::{Finish, Kind, Outcome, Recorder};
use std::sync::Arc;

/// Static assets are served by fixed handlers and are not business work.
fn observable(route: &str) -> bool {
    !["/assets/", "/fonts/", "/favicon"]
        .iter()
        .any(|prefix| route.starts_with(prefix))
}

/// Records every matched route under its template (`/companies/{id}`), never
/// the concrete path or query. Unmatched requests have no template, so none is
/// recorded.
pub async fn requests(
    State(recorder): State<Arc<Recorder>>,
    request: Request,
    next: Next,
) -> Response {
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|path| path.as_str().to_owned())
        .filter(|route| observable(route));
    recorder
        .observe(
            Kind::Request,
            route.as_deref(),
            next.run(request),
            |response| Finish {
                outcome: if response.status().as_u16() < 400 {
                    Outcome::Ok
                } else {
                    Outcome::Error
                },
                response_bytes: response.body().size_hint().exact(),
            },
        )
        .await
}
