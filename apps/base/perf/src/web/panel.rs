//! The panel checks authorization before its cache, so a revoked grant cannot
//! expose a previous read. A permission refusal produces the unloaded strip.
use super::{view::render, Pages, PATH};
use crate::{Diagnostics, Health, Store};
use askama::Template;
use bilibi_dashboard::{PanelArgs, PanelCache};
use bilibi_database::{Error, Result};
use bilibi_tenancy::Scope;
use bilibi_ui::{
    page::{LiveBody, LiveFuture, LiveInput, LiveRequest, LiveState, PageRequest},
    Html, StatItem, StatStrip,
};

pub(super) struct HealthPanel {
    pages: Pages,
    args: PanelArgs,
    cache: PanelCache,
}
impl HealthPanel {
    pub fn new(pages: &Pages, request: &PageRequest<'_, Scope>) -> Result<Self> {
        Ok(Self {
            pages: pages.clone(),
            args: PanelArgs::from_request(request)?,
            cache: PanelCache::default(),
        })
    }
}
#[derive(Template)]
#[template(source = r#"<div id="{{ id }}">{{ strip|safe }}</div>"#, ext = "html")]
struct Root<'a> {
    id: &'a str,
    strip: Html,
}
fn strip(args: &PanelArgs, diagnostics: Option<&Diagnostics>, allowed: bool) -> Result<String> {
    let health = match diagnostics {
        None => "—",
        Some(d) if d.store == Store::Unavailable => "History unavailable",
        Some(d) => match d.recorder {
            Health::Unavailable => "Recorder unavailable",
            Health::Degraded => "Degraded",
            Health::Available => "Available",
        },
    };
    let samples = diagnostics
        .and_then(|d| d.samples)
        .map_or_else(|| "—".into(), |s| s.to_string());
    Ok(render(Root {
        id: &args.id(),
        strip: render(StatStrip {
            id: "stat-performance",
            title: "Performance",
            href: if allowed && !args.editing { PATH } else { "" },
            items: vec![
                StatItem::text("Health", health),
                StatItem::number("Samples", samples),
            ],
        })?,
    })?
    .as_str()
    .to_owned())
}
impl LiveState<Scope, Error> for HealthPanel {
    fn update<'a>(&'a mut self, request: LiveRequest<'a, Scope>) -> LiveFuture<'a, Error> {
        Box::pin(async move {
            if !matches!(request.input, LiveInput::Refresh) {
                return Err(Error::Invalid);
            }
            let html = match self.pages.authorize(request.scope).await {
                Err(Error::Forbidden) => {
                    self.cache = PanelCache::default();
                    strip(&self.args, None, false)?
                }
                Err(error) => return Err(error),
                Ok(()) => {
                    if let Some(html) = self.cache.fresh(&self.args) {
                        html.to_owned()
                    } else {
                        let diagnostics = self.pages.recorder.diagnostics().await;
                        self.cache
                            .store(strip(&self.args, Some(&diagnostics), true)?)
                    }
                }
            };
            Ok(LiveBody {
                html,
                ..LiveBody::default()
            })
        })
    }
}
