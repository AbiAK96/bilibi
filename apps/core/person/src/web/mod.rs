//! Person-owned screens. Contacts work independently of People or accounts.
mod create;
mod detail;
mod index;
mod panels;

use crate::{PersonId, PersonService};
use askama::Template;
use bilibi_authz::effective_capabilities;
use bilibi_database::{Db, Error, Result};
use bilibi_datetime::{DisplayContext, DisplayPolicy};
use bilibi_grid::PageViews;
use bilibi_locale::Locales;
use bilibi_tenancy::Scope;
use bilibi_ui::{page::*, Html};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};

pub const SERVICE: &str = "core/person.pages";
#[derive(Clone)]
pub struct Pages {
    pub db: Db,
    pub persons: Arc<PersonService>,
    pub views: Arc<PageViews>,
    pub display: Arc<DisplayPolicy>,
    pub locales: Arc<Locales>,
    pub person_panels: Vec<(String, String)>,
}
impl Pages {
    async fn allowed(&self, scope: &Scope, capability: &str) -> Result<bool> {
        let client = self.db.get().await?;
        Ok(effective_capabilities(&**client, scope)
            .await?
            .contains(capability))
    }
    async fn display(&self, scope: &Scope) -> Result<DisplayContext> {
        let client = self.db.get().await?;
        let locale = self.locales.resolve(&**client, scope, None).await?.locale;
        self.display.display(&**client, scope, None, locale).await
    }
    async fn state(
        &self,
        request: &PageRequest<'_, Scope>,
    ) -> Result<Box<dyn LiveState<Scope, Error>>> {
        let scope = request.scope.ok_or(Error::Unauthorized)?;
        match request.operation {
            "list" => Ok(Box::new(
                index::Index::new(self.clone(), scope, request.query, request.framed).await?,
            )),
            "create" => Ok(Box::new(create::Create::new(self.clone()))),
            "detail" => Ok(Box::new(detail::Detail::new(
                self.clone(),
                id(request.path)?,
                request.framed,
            ))),
            "contact" => Ok(Box::new(panels::Contact::new(
                self.clone(),
                id(request.path)?,
                request.path.get("panel").ok_or(Error::Invalid)?.clone(),
                request.framed,
            ))),
            _ => Err(Error::NotFound),
        }
    }
}
impl Page<Scope, Error> for Pages {
    fn title(&self, operation: &str) -> Option<&'static str> {
        Some(match operation {
            "list" => "People",
            "create" => "New Person",
            _ => "Person",
        })
    }
    fn render<'a>(&'a self, request: PageRequest<'a, Scope>) -> PageFuture<'a, Error> {
        Box::pin(async move {
            let scope = request.scope.ok_or(Error::Unauthorized)?;
            let mut state = self.state(&request).await?;
            let body = state
                .update(LiveRequest {
                    scope,
                    credential: "",
                    input: LiveInput::Refresh,
                })
                .await?;
            Ok(PageBody {
                html: body.html,
                live: true,
            })
        })
    }
    fn mount<'a>(&'a self, request: PageRequest<'a, Scope>) -> MountFuture<'a, Scope, Error> {
        Box::pin(async move { Ok(Some(self.state(&request).await?)) })
    }
}
fn id(values: &HashMap<String, String>) -> Result<PersonId> {
    values
        .get("id")
        .and_then(|id| id.parse().ok())
        .and_then(|id| PersonId::new(id).ok())
        .ok_or(Error::Invalid)
}
fn render(template: impl Template) -> Result<Html> {
    Html::render(&template).map_err(|_| Error::Internal)
}
fn values(event: &Value) -> HashMap<String, String> {
    event
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(key, value)| {
            let value = match value {
                Value::String(text) => text.clone(),
                Value::Number(number) => number.to_string(),
                _ => return None,
            };
            Some((key.clone(), value))
        })
        .collect()
}
fn event(event: &Value) -> Result<&str> {
    event
        .get("type")
        .and_then(Value::as_str)
        .ok_or(Error::Invalid)
}

/// Discover owner panels without depending on Address, Affiliation, User or
/// People. A missing optional owner contributes nothing; the host reauthorizes
/// each contributed panel independently on mount, event and refresh.
pub fn person_panels(
    services: &bilibi_module_registry::Services,
) -> bilibi_module_registry::Result<Vec<(String, String)>> {
    use bilibi_module_registry::contributions::{Embed, EmbedContext, EMBEDS};
    let mut panels = Vec::new();
    for entry in services.catalog()?.entries::<Embed>(EMBEDS)? {
        let embed = entry.get::<Embed>()?;
        if matches!(
            embed.context,
            EmbedContext::Record {
                kind: "core/person"
            }
        ) && entry.id != "person.contacts"
        {
            panels.push((
                format!("person-panel-{}", entry.id.replace(['.', '/'], "-")),
                entry.id.into(),
            ));
        }
    }
    Ok(panels)
}
