//! Company owns its page state, Grid arrangement and all business events.
mod views;
use crate::{lifecycle::Operation, CreateOutcome, Edit, FieldError, Params};
use bilibi_database::{Db, Error, Result};
use bilibi_grid::{PageColumns, PageViews};
use bilibi_tenancy::Scope;
use bilibi_ui::{page::*, CommitStates, CommitStatus};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};

pub struct Pages {
    pub db: Db,
    pub company: Arc<crate::CompanyService>,
    pub views: Arc<PageViews>,
    pub datetime: Arc<bilibi_datetime::DisplayPolicy>,
}
impl Pages {
    async fn state(&self, request: &PageRequest<'_, Scope>) -> Result<CompanyPage> {
        let scope = request.scope.ok_or(Error::Unauthorized)?;
        let id = match request.operation {
            "list" | "create" => None,
            "detail" => Some(
                request
                    .path
                    .get("id")
                    .and_then(|v| v.parse().ok())
                    .filter(|id: &i64| *id > 0)
                    .ok_or(Error::Invalid)?,
            ),
            _ => return Err(Error::NotFound),
        };
        let client = self.db.get().await?;
        let masked = bilibi_authz::restrictions(&**client, scope).await?;
        let mut columns = views::columns(self.views.clone(), &masked)?;
        columns.from_params(scope, request.query).await?;
        Ok(CompanyPage {
            db: self.db.clone(),
            company: self.company.clone(),
            datetime: self.datetime.clone(),
            operation: request.operation,
            id,
            params: params(request.query)?,
            columns,
            framed: request.framed,
            statuses: CommitStates::default(),
            views: self.views.clone(),
            masked: masked.clone(),
            form: HashMap::from([("status".into(), "active".into())]),
            errors: Vec::new(),
            pending: None,
            notice: String::new(),
        })
    }
}
impl Page<Scope, Error> for Pages {
    fn title(&self, operation: &str) -> Option<&'static str> {
        Some(match operation {
            "create" => "Create Company",
            "detail" => "Company",
            _ => "Companies",
        })
    }
    fn render<'a>(&'a self, request: PageRequest<'a, Scope>) -> PageFuture<'a, Error> {
        Box::pin(async move {
            let mut state = self.state(&request).await?;
            let body = state
                .draw(request.scope.ok_or(Error::Unauthorized)?)
                .await?;
            Ok(PageBody {
                html: body.html,
                live: true,
            })
        })
    }
    fn mount<'a>(&'a self, request: PageRequest<'a, Scope>) -> MountFuture<'a, Scope, Error> {
        Box::pin(async move {
            Ok(Some(
                Box::new(self.state(&request).await?) as Box<dyn LiveState<Scope, Error>>
            ))
        })
    }
}

pub fn params(values: &HashMap<String, String>) -> Result<Params> {
    let mut result = Params::default();
    for (key, value) in values {
        match key.as_str() {
            "page" => result.page = value.parse().unwrap_or(1).clamp(1, 100_000),
            "size" | "per_page" | "perPage" => {
                result.size = value
                    .parse()
                    .ok()
                    .filter(|v| [25, 50, 100, 300].contains(v))
                    .unwrap_or(25)
            }
            "sort" => {
                result.sort =
                    if ["name", "status", "jurisdiction", "created_at"].contains(&value.as_str()) {
                        value.clone()
                    } else {
                        "name".into()
                    }
            }
            "dir" => {
                result.dir = if value == "desc" {
                    "desc".into()
                } else {
                    "asc".into()
                }
            }
            "q" | "search" => result.q = value.trim().chars().take(255).collect(),
            "status" | "status_filter" => {
                result.status = if crate::organisation::STATUSES.contains(&value.as_str()) {
                    value.clone()
                } else {
                    String::new()
                }
            }
            "cols" | "lens" | "z" | "since" => {}
            _ => return Err(Error::Invalid),
        }
    }
    result.validate()?;
    Ok(result)
}
struct Pending {
    operation: Operation,
    version: i64,
    reason: String,
    error: String,
}
struct CompanyPage {
    db: Db,
    company: Arc<crate::CompanyService>,
    datetime: Arc<bilibi_datetime::DisplayPolicy>,
    operation: &'static str,
    id: Option<i64>,
    params: Params,
    columns: PageColumns,
    framed: bool,
    statuses: CommitStates,
    views: Arc<PageViews>,
    masked: std::collections::HashSet<String>,
    form: HashMap<String, String>,
    errors: Vec<FieldError>,
    pending: Option<Pending>,
    notice: String,
}
impl CompanyPage {
    fn query(&self) -> HashMap<String, String> {
        let mut values = self.columns.params();
        let p = &self.params;
        for (key, value, default) in [
            ("page", p.page.to_string(), "1"),
            ("per_page", p.size.to_string(), "25"),
            ("sort", p.sort.clone(), "name"),
            ("dir", p.dir.clone(), "asc"),
            ("search", p.q.clone(), ""),
            ("status", p.status.clone(), ""),
        ] {
            if value != default {
                values.insert(key.into(), value);
            }
        }
        values
    }
    async fn draw(&mut self, scope: &Scope) -> Result<LiveBody> {
        let client = self.db.get().await?;
        let masked = bilibi_authz::restrictions(&**client, scope).await?;
        let html = match self.operation {
            "list" => {
                if masked.contains(&self.params.sort) {
                    self.params.sort = "name".into();
                    self.params.dir = "asc".into();
                }
                let mut page = crate::list(&**client, scope, &self.params).await?;
                let last = ((page.total + self.params.size - 1) / self.params.size).max(1);
                if self.params.page > last {
                    self.params.page = last;
                    page = crate::list(&**client, scope, &self.params).await?;
                }
                // Rebuild built-ins under the fresh restrictions too: a retained
                // built-in must never outlive its reader's field access.
                if masked != self.masked {
                    let query = self.columns.params();
                    self.columns = views::columns(self.views.clone(), &masked)?;
                    self.columns.from_params(scope, &query).await?;
                    self.masked = masked.clone();
                }
                let display = self
                    .datetime
                    .display(&**client, scope, None, "en-MY")
                    .await?;
                self.columns
                    .load(
                        scope,
                        &page
                            .rows
                            .iter()
                            .map(|row| serde_json::json!(row.id))
                            .collect::<Vec<_>>(),
                        &display,
                    )
                    .await?;
                views::list(
                    &**client,
                    scope,
                    &page,
                    &self.params,
                    &self.columns,
                    self.framed,
                    &masked,
                )
                .await?
            }
            "detail" => {
                views::detail(
                    &**client,
                    scope,
                    self.id.ok_or(Error::Invalid)?,
                    &self.statuses,
                    self.pending.as_ref(),
                    &self.notice,
                )
                .await?
            }
            "create" => views::create(&**client, scope, &self.form, &self.errors).await?,
            _ => return Err(Error::NotFound),
        };
        Ok(LiveBody {
            html,
            query: if self.operation == "list" {
                self.query()
            } else {
                HashMap::new()
            },
            notice: self.notice.clone(),
            rows: 1,
            ..LiveBody::default()
        })
    }
    fn sort(&mut self, field: &str) -> Result<()> {
        if self.operation != "list"
            || !["name", "status", "jurisdiction", "created_at"].contains(&field)
        {
            return Err(Error::Invalid);
        }
        self.params.dir = if self.params.sort == field && self.params.dir == "asc" {
            "desc"
        } else {
            "asc"
        }
        .into();
        self.params.sort = field.into();
        self.params.page = 1;
        Ok(())
    }
    async fn event(
        &mut self,
        scope: &Scope,
        credential: &str,
        value: &Value,
    ) -> Result<Option<String>> {
        let get = |key| value.get(key).and_then(Value::as_str).unwrap_or("");
        match get("type") {
            "grid" if self.operation == "list" => {
                let values = value
                    .as_object()
                    .ok_or(Error::Invalid)?
                    .iter()
                    .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.into())))
                    .collect();
                if let bilibi_grid::page_columns::Outcome::Sort(field) =
                    self.columns.handle(scope, &values).await?
                {
                    self.sort(&field)?;
                }
            }
            "sort" => self.sort(if get("sort").is_empty() {
                get("field")
            } else {
                get("sort")
            })?,
            "filters" if self.operation == "list" => {
                for (key, field) in [
                    ("filters[search]", "search"),
                    ("filters[status_filter]", "status"),
                    ("filters[perPage]", "per_page"),
                ] {
                    if let Some(value) = value.get(key).and_then(Value::as_str) {
                        let mut query = self.query();
                        query.insert(field.into(), value.into());
                        query.remove("page");
                        self.params = params(&query)?;
                    }
                }
                self.params.page = 1;
            }
            "page" if self.operation == "list" => {
                self.params.page = get("page").parse().unwrap_or(1).clamp(1, 100_000);
            }
            "clear_filters" if self.operation == "list" => {
                self.params.q.clear();
                self.params.status.clear();
                self.params.page = 1;
            }
            "save_field" if self.operation == "detail" => {
                let id = get("id").parse().map_err(|_| Error::Invalid)?;
                if self.id != Some(id) {
                    return Err(Error::Invalid);
                }
                let edit = Edit {
                    id,
                    field: get("field").into(),
                    value: get("value").into(),
                    version: get("version").parse().map_err(|_| Error::Invalid)?,
                };
                let result = self.company.save_fact(scope, credential, &edit).await;
                let status = match result {
                    Ok(()) => CommitStatus::Saved,
                    Err(Error::Unauthorized) => return Err(Error::Unauthorized),
                    Err(Error::Forbidden) => {
                        self.notice="You no longer have permission to change this company, or the company is archived.".into();
                        CommitStatus::Error(self.notice.clone())
                    }
                    Err(Error::Conflict) => CommitStatus::Error(
                        "The record changed. Your draft is kept; reload before saving.".into(),
                    ),
                    Err(Error::Invalid) => CommitStatus::Error(
                        "The value was not saved. Check its format, length and references.".into(),
                    ),
                    Err(_) => CommitStatus::failure(),
                };
                self.statuses.put(edit.field, status);
            }
            "request_lifecycle" if self.operation == "detail" => {
                let op: Operation = serde_json::from_value(Value::String(get("operation").into()))
                    .map_err(|_| Error::Invalid)?;
                let client = self.db.get().await?;
                let id = self.id.ok_or(Error::Invalid)?;
                crate::organisation::authorize_target(&**client, scope, id, crate::UPDATE).await?;
                let (company, _) = crate::detail(&**client, scope, id).await?;
                if !op.permits(&company.status) {
                    return Err(Error::Invalid);
                }
                self.pending = Some(Pending {
                    operation: op,
                    version: company.version,
                    reason: String::new(),
                    error: String::new(),
                });
            }
            "cancel_lifecycle" => self.pending = None,
            "apply_lifecycle" if self.operation == "detail" => {
                let pending = self.pending.as_mut().ok_or(Error::Invalid)?;
                pending.reason = get("reason").into();
                match self
                    .company
                    .transition_company(
                        scope,
                        self.id.ok_or(Error::Invalid)?,
                        pending.operation,
                        Some(&pending.reason),
                        Some(pending.version),
                    )
                    .await
                {
                    Ok(_) => {
                        self.pending = None;
                        self.notice = "Company status changed.".into();
                    }
                    Err(crate::lifecycle::LifecycleError::Write(crate::WriteError::Access(
                        Error::Unauthorized,
                    ))) => return Err(Error::Unauthorized),
                    Err(error) => pending.error = error.to_string(),
                }
            }
            "create_company" if self.operation == "create" => {
                self.form = value
                    .as_object()
                    .ok_or(Error::Invalid)?
                    .iter()
                    .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.into())))
                    .filter(|(k, _)| k != "type")
                    .collect();
                let (new, activities, metadata, errors) = views::new_company(&self.form);
                self.errors = errors;
                if self.errors.is_empty() {
                    match self
                        .company
                        .create_with_context(scope, &new, activities, metadata)
                        .await?
                    {
                        CreateOutcome::Created(_) => return Ok(Some("/companies".into())),
                        CreateOutcome::Refused(errors) => self.errors = errors,
                    }
                }
            }
            _ => return Err(Error::Invalid),
        }
        Ok(None)
    }
}
impl LiveState<Scope, Error> for CompanyPage {
    fn update<'a>(&'a mut self, request: LiveRequest<'a, Scope>) -> LiveFuture<'a, Error> {
        Box::pin(async move {
            match request.input {
                LiveInput::Refresh => {}
                LiveInput::Params(values) => {
                    self.params = params(values)?;
                    self.columns.from_params(request.scope, values).await?;
                }
                LiveInput::Event(value) => {
                    if let Some(url) = self.event(request.scope, request.credential, value).await? {
                        return Ok(LiveBody {
                            redirect: Some(url),
                            ..LiveBody::default()
                        });
                    }
                }
            }
            self.draw(request.scope).await
        })
    }
    fn panels(&self) -> Vec<PanelSpec> {
        self.id
            .map(|id| {
                vec![PanelSpec {
                    id: "company-record-history".into(),
                    embed: "audit.record-history".into(),
                    path: [
                        ("panel".into(), "company-record-history".into()),
                        ("kinds".into(), "company".into()),
                        ("id".into(), id.to_string()),
                    ]
                    .into(),
                    query: HashMap::new(),
                }]
            })
            .unwrap_or_default()
    }
}
