use crate::{Company, Page, Params};
use askama::Template;

#[derive(Template)]
#[template(path = "list.html")]
pub struct List<'a> {
    pub page: &'a Page,
    pub params: &'a Params,
    pub pages: i64,
}

pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub value: String,
    pub masked: bool,
}
#[derive(Template)]
#[template(path = "detail.html")]
pub struct Detail<'a> {
    pub company: &'a Company,
    pub fields: Vec<Field>,
    pub editable: bool,
}
impl<'a> Detail<'a> {
    pub fn new(
        company: &'a Company,
        masked: &std::collections::HashSet<String>,
        editable: bool,
    ) -> Self {
        Self {
            company,
            editable: editable && company.status != "archived",
            fields: vec![
                ("name", "Name", &company.name),
                ("status", "Status", &company.status),
                ("email", "Email", &company.email),
                ("phone", "Phone", &company.phone),
                ("tax_id", "Tax ID", &company.tax_id),
                ("revenue", "Revenue", &company.revenue),
            ]
            .into_iter()
            .map(|(key, label, value)| Field {
                key,
                label,
                value: value.clone(),
                masked: masked.contains(key),
            })
            .collect(),
        }
    }
}

/// HTTP page ownership stays with Company; the host knows only PageService.
pub struct Pages {
    pub db: bilibi_database::Db,
    pub company: std::sync::Arc<crate::CompanyService>,
}
impl bilibi_ui::page::Page<bilibi_security::Scope, bilibi_database::Error> for Pages {
    fn render<'a>(
        &'a self,
        request: bilibi_ui::page::PageRequest<'a, bilibi_security::Scope>,
    ) -> bilibi_ui::page::PageFuture<'a, bilibi_database::Error> {
        Box::pin(async move {
            use bilibi_database::Error;
            use bilibi_security::{authorize, Capability};
            let scope = request.scope.ok_or(Error::Unauthorized)?;
            let client = self.db.get().await?;
            let html = match request.operation {
                "list" => {
                    let params = params(request.query)?;
                    let page = crate::list(&**client, scope, &params).await?;
                    List {
                        pages: ((page.total + params.size - 1) / params.size).max(1),
                        page: &page,
                        params: &params,
                    }
                    .render()
                    .map_err(|_| Error::Internal)?
                }
                "detail" => {
                    if !request.query.is_empty() {
                        return Err(Error::Invalid);
                    }
                    let id = request
                        .path
                        .get("id")
                        .and_then(|id| id.parse().ok())
                        .filter(|id: &i64| *id > 0)
                        .ok_or(Error::Invalid)?;
                    let (company, masked) = crate::detail(&**client, scope, id).await?;
                    let editable =
                        match authorize(&**client, scope, Capability::CompaniesUpdate).await {
                            Ok(()) => true,
                            Err(Error::Forbidden) => false,
                            Err(error) => return Err(error),
                        };
                    Detail::new(&company, &masked, editable)
                        .render()
                        .map_err(|_| Error::Internal)?
                }
                _ => return Err(Error::NotFound),
            };
            Ok(bilibi_ui::page::PageBody { html, live: true })
        })
    }
    fn mount<'a>(
        &'a self,
        request: bilibi_ui::page::PageRequest<'a, bilibi_security::Scope>,
    ) -> bilibi_ui::page::MountFuture<'a, bilibi_security::Scope, bilibi_database::Error> {
        Box::pin(async move {
            let id = match request.operation {
                "list" => None,
                "detail" => Some(
                    request
                        .path
                        .get("id")
                        .and_then(|v| v.parse::<i64>().ok())
                        .filter(|v| *v > 0)
                        .ok_or(bilibi_database::Error::Invalid)?,
                ),
                _ => return Err(bilibi_database::Error::NotFound),
            };
            if id.is_some() && !request.query.is_empty() {
                return Err(bilibi_database::Error::Invalid);
            }
            let state: Box<
                dyn bilibi_ui::page::LiveState<bilibi_security::Scope, bilibi_database::Error>,
            > = Box::new(CompanyPage {
                db: self.db.clone(),
                company: self.company.clone(),
                id,
                params: params(request.query)?,
            });
            Ok(Some(state))
        })
    }
}

pub fn params(
    values: &std::collections::HashMap<String, String>,
) -> bilibi_database::Result<Params> {
    use bilibi_database::Error;
    let mut result = Params::default();
    for (key, value) in values {
        match key.as_str() {
            "page" => result.page = value.parse().map_err(|_| Error::Invalid)?,
            "size" => result.size = value.parse().map_err(|_| Error::Invalid)?,
            "sort" => result.sort = value.clone(),
            "dir" => result.dir = value.clone(),
            "q" => result.q = value.clone(),
            "status" => result.status = value.clone(),
            _ => return Err(Error::Invalid),
        }
    }
    result.validate()?;
    Ok(result)
}

/// The Company owner retains typed list/edit state; no Company protocol lives in Web.
struct CompanyPage {
    db: bilibi_database::Db,
    company: std::sync::Arc<crate::CompanyService>,
    id: Option<i64>,
    params: Params,
}
#[derive(serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum CompanyEvent {
    Sort { field: String },
    Page { delta: i64 },
    Save { edit: crate::Edit },
}
impl bilibi_ui::page::LiveState<bilibi_security::Scope, bilibi_database::Error> for CompanyPage {
    fn update<'a>(
        &'a mut self,
        request: bilibi_ui::page::LiveRequest<'a, bilibi_security::Scope>,
    ) -> bilibi_ui::page::LiveFuture<'a, bilibi_database::Error> {
        Box::pin(async move {
            use bilibi_database::Error;
            use bilibi_ui::page::{LiveBody, LiveInput};
            let mut next = self.params.clone();
            let mut notice = String::new();
            match request.input {
                LiveInput::Refresh => {}
                LiveInput::Params(values) => {
                    if self.id.is_some() && !values.is_empty() {
                        return Err(Error::Invalid);
                    }
                    next = params(values)?;
                }
                LiveInput::Event(value) => {
                    match serde_json::from_value::<CompanyEvent>(value.clone())
                        .map_err(|_| Error::Invalid)?
                    {
                        CompanyEvent::Sort { field } => {
                            if self.id.is_some()
                                || !["name", "status", "created_at"].contains(&field.as_str())
                            {
                                return Err(Error::Invalid);
                            }
                            next.dir = if next.sort == field && next.dir == "asc" {
                                "desc"
                            } else {
                                "asc"
                            }
                            .into();
                            next.sort = field;
                            next.page = 1;
                        }
                        CompanyEvent::Page { delta } => {
                            if self.id.is_some() || ![-1, 1].contains(&delta) {
                                return Err(Error::Invalid);
                            }
                            next.page = (next.page + delta).max(1);
                            next.validate()?;
                        }
                        CompanyEvent::Save { edit } => {
                            if self.id != Some(edit.id) {
                                return Err(Error::Invalid);
                            }
                            self.company.save(request.credential, &edit).await?;
                            notice = "Saved · audit recorded".into();
                        }
                    }
                }
            }
            let client = self.db.get().await?;
            let (html, rows, query) = if let Some(id) = self.id {
                let (company, masked) = crate::detail(&**client, request.scope, id).await?;
                let editable = match bilibi_security::authorize(
                    &**client,
                    request.scope,
                    bilibi_security::Capability::CompaniesUpdate,
                )
                .await
                {
                    Ok(()) => true,
                    Err(Error::Forbidden) => false,
                    Err(error) => return Err(error),
                };
                (
                    Detail::new(&company, &masked, editable)
                        .render()
                        .map_err(|_| Error::Internal)?,
                    1,
                    std::collections::HashMap::new(),
                )
            } else {
                let page = crate::list(&**client, request.scope, &next).await?;
                let html = List {
                    pages: ((page.total + next.size - 1) / next.size).max(1),
                    page: &page,
                    params: &next,
                }
                .render()
                .map_err(|_| Error::Internal)?;
                let query = [
                    ("page", next.page.to_string()),
                    ("size", next.size.to_string()),
                    ("sort", next.sort.clone()),
                    ("dir", next.dir.clone()),
                    ("q", next.q.clone()),
                    ("status", next.status.clone()),
                ]
                .into_iter()
                .map(|(k, v)| (k.into(), v))
                .collect();
                (html, page.rows.len(), query)
            };
            self.params = next;
            Ok(LiveBody {
                html,
                query,
                notice,
                rows,
                redirect: None,
            })
        })
    }
}
