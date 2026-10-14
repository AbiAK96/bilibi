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
