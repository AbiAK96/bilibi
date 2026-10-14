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
