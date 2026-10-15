//! Role administration screens (`base/authz.pages`), ported from Bilimbi
//! `apps/base/authz/lib/authz/web/{roles_index,role_create,role_show}_live`:
//!
//! - `/authz/roles` (operation `roles`, `admin.authz.role.list`): the tenant's
//!   roles, searchable, sortable and paged in SQL.
//! - `/authz/roles/create` (`role-create`, `admin.authz.role.create`): a new
//!   custom role with no capabilities, landing on its detail page.
//! - `/authz/roles/{id}` (`role`, `admin.authz.role.view`): one role, the
//!   capabilities it grants and the principals holding it.
//!
//! The route guard checks each capability on every request and live event.
//! Bilimbi asks for an owning company on create and labels company-less custom
//! roles "Unowned"; Bilibi keeps custom roles in the tenant until K15 binds
//! them to a company, so the picker, the Unowned label and the operator's
//! principal-count caution have nothing to show yet. Bilimbi's flexible table
//! is the shared table until FlexTable (U7) lands.
use crate::{
    grants::CreatedRole,
    roles::{self, NewRole, Query, RoleDetails, RoleErrors, RoleSummary, Sort},
    web::{render, Pages, TextCell},
};
use askama::Template;
use bilibi_database::{Error, Result};
use bilibi_tenancy::Scope;
use bilibi_ui::{
    list_state::FILTERS_FORM,
    page::{LiveBody, LiveFuture, LiveInput, LiveRequest, LiveState},
    params, BackLink, Badge, BadgeKind, Button, ButtonVariant, Card, Column, CreateButton, Fact,
    Facts, Field, FilterToolbar, FlashGroup, FlashMap, Header, Html, InputKind, ListSpec,
    ListState, PageCounts, PageVariant, Pagination, Row, SectionHeading, SortDir, Table,
    TableEmpty, ToolbarControl,
};
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};

const PAGE_SIZES: [u64; 4] = [25, 50, 100, 300];

/// Every column opens ascending; an unrecognised rows-per-page value falls
/// back to 25 and the URL key stays `page_size`.
static SPEC: LazyLock<Arc<ListSpec>> = LazyLock::new(|| {
    ListSpec {
        sortable: ["name", "code", "is_system", "created_at"]
            .into_iter()
            .map(|key| (key.to_owned(), SortDir::Asc))
            .collect(),
        default_sort: Some("name".into()),
        page_sizes: PAGE_SIZES.to_vec(),
        page_size_param: "page_size".into(),
        invalid_page_size: bilibi_ui::list_state::InvalidPageSize::Default,
        ..ListSpec::default()
    }
    .shared()
    .expect("role list spec")
});

pub(crate) fn parse(values: &HashMap<String, String>) -> Result<ListState> {
    ListState::parse(values, SPEC.clone()).map_err(|_| Error::Internal)
}

fn query(state: &ListState) -> Query {
    Query {
        search: state.search.clone(),
        sort: state
            .sort_by
            .as_deref()
            .and_then(Sort::parse)
            .unwrap_or(Sort::Name),
        descending: state.sort_dir == Some(SortDir::Desc),
        page: state.page,
        page_size: state.page_size,
    }
}

/// System roles ship with the platform; every other role is the tenant's own.
fn scope_badge(role: &RoleSummary) -> Badge<'static> {
    if role.is_system {
        Badge::status(BadgeKind::Neutral, "System")
    } else {
        Badge::status(BadgeKind::Success, "Custom")
    }
}

#[derive(Template)]
#[template(
    source = r#"<a href="/authz/roles/{{ id }}" class="font-medium text-ink-strong hover:underline">{{ name }}</a>{% if !description.is_empty() %}<p class="text-xs text-ink-subtle">{{ description }}</p>{% endif %}"#,
    ext = "html"
)]
struct NameCell<'a> {
    id: i64,
    name: &'a str,
    description: &'a str,
}

#[derive(Template)]
#[template(
    source = r#"{{ scope }}{% if grant_all %}{{ grants_all }}{% endif %}"#,
    ext = "html"
)]
struct ScopeCell {
    scope: Html,
    grant_all: bool,
    grants_all: Html,
}

#[derive(Template)]
#[template(
    source = r#"<span class="tabular-nums text-ink-muted">{% if grant_all %}<span title="A grant-all role carries every capability, present and future">All</span>{% else %}{{ count }}{% endif %}</span>"#,
    ext = "html"
)]
struct CapabilityCountCell {
    grant_all: bool,
    count: i64,
}

fn role_row(role: &RoleSummary) -> Result<Row> {
    let mut grants_all = Badge::status(BadgeKind::Warning, "Grants all");
    grants_all.class = "ml-1";
    Ok(Row {
        id: format!("roles-{}", role.id),
        cells: vec![
            render(NameCell {
                id: role.id,
                name: &role.name,
                description: &role.description,
            })?,
            render(TextCell {
                class: "font-mono text-xs text-ink-muted",
                text: role.code.as_deref().unwrap_or_default(),
            })?,
            render(ScopeCell {
                scope: render(scope_badge(role))?,
                grant_all: role.grant_all,
                grants_all: render(grants_all)?,
            })?,
            render(CapabilityCountCell {
                grant_all: role.grant_all,
                count: role.capability_count,
            })?,
            render(TextCell {
                class: "tabular-nums text-ink-muted",
                text: &role.principal_count.to_string(),
            })?,
        ],
        actions: Html::EMPTY,
    })
}

impl Pages {
    pub(crate) async fn roles_view(&self, scope: &Scope, state: &mut ListState) -> Result<String> {
        let client = self.db.get().await?;
        let can_create = crate::decide(&**client, scope, roles::CREATE)
            .await?
            .allowed;
        let mut page = roles::list(&**client, scope, &query(state)).await?;
        // An out-of-range page (a stale link, a narrower search) lands on the
        // last real page instead of an empty one with no pager.
        if state.clamp_to_last_page(page.total_entries.div_ceil(state.page_size), true) {
            page = roles::list(&**client, scope, &query(state)).await?;
        }
        let actions = if can_create {
            render(CreateButton {
                noun: "Role",
                href: "/authz/roles/create",
                id: "roles-create",
                ..CreateButton::default()
            })?
        } else {
            Html::EMPTY
        };
        let mut html = render(Header {
            title: Html::text("Roles"),
            subtitle: "System roles ship with the platform; custom roles belong to this tenant",
            actions,
            ..Header::default()
        })?;
        let form = state.filters_form();
        let mut toolbar = FilterToolbar::new(
            "roles-filters",
            vec![ToolbarControl::search(
                "roles-search",
                "search",
                "Search roles",
                &form["search"],
                "Search name, code or description",
            )],
        );
        toolbar.event = "search";
        html.push(&render(toolbar)?);
        let mut table = Table::new(
            "roles",
            vec![
                Column::new("Name").sort("name").sort_id("roles-sort-name"),
                Column::new("Code").sort("code").sort_id("roles-sort-code"),
                Column::new("Scope")
                    .sort("is_system")
                    .sort_id("roles-sort-is_system"),
                Column::new("Capabilities").right(),
                Column::new("Principals").right(),
            ],
        )
        .sorted(state.sort_by.as_deref(), state.sort_dir);
        table.caption = "Roles";
        table.framed = false;
        for role in &page.entries {
            table.rows.push(role_row(role)?);
        }
        if page.total_entries == 0 {
            table.empty = Some(TableEmpty::Content(Html::text(
                &if state.search.is_empty() {
                    "No roles are visible in this tenant.".to_owned()
                } else {
                    format!("No roles match {}.", state.search)
                },
            )));
        }
        let mut card = render(table)?;
        let mut pagination = Pagination::new(
            "roles-pagination",
            PageCounts::new(state.page, state.page_size, page.total_entries),
            &PAGE_SIZES,
        );
        pagination.filters_event = "search";
        card.push(&render(pagination)?);
        html.push(&render(Card {
            id: "roles-card",
            inner_class: "p-0",
            body: card,
            ..Card::default()
        })?);
        Ok(render(bilibi_ui::Page {
            id: "roles-index",
            body: html,
            ..bilibi_ui::Page::default()
        })?
        .as_str()
        .to_owned())
    }

    pub(crate) async fn role_view(&self, scope: &Scope, id: i64) -> Result<String> {
        let client = self.db.get().await?;
        // Another tenant's id and a missing id are the same answer.
        let details = roles::get(&**client, scope, id)
            .await?
            .ok_or(Error::NotFound)?;
        render_role(&details)
    }
}

#[derive(Template)]
#[template(
    source = r#"{% if grant_all %}<p id="role-grant-all" class="mt-1 text-xs text-ink-subtle">This role grants every capability, including ones added by modules installed later. Individual grants below are therefore not the whole story.</p>{% endif %}{% if !capabilities.is_empty() %}<ul id="role-capabilities" class="mt-2 flex flex-wrap gap-1.5">{% for capability in capabilities %}<li class="rounded-md bg-surface-sunken px-2 py-1 font-mono text-xs text-ink-muted">{{ capability }}</li>{% endfor %}</ul>{% elif !grant_all %}<p class="mt-2 rounded-xl border border-line bg-surface px-4 py-6 text-center text-sm text-ink-muted">This role grants no capabilities.</p>{% endif %}"#,
    ext = "html"
)]
struct CapabilityList<'a> {
    grant_all: bool,
    capabilities: &'a [String],
}

fn section(id: &str, card: &str, title: &str, body: Html) -> Result<Html> {
    let mut inner = render(SectionHeading {
        id,
        title,
        ..SectionHeading::default()
    })?;
    inner.push(&body);
    render(Card {
        id: card,
        class: "mt-6",
        inner_class: "p-5 sm:p-6",
        body: inner,
        ..Card::default()
    })
}

fn render_role(details: &RoleDetails) -> Result<String> {
    let role = &details.role;
    let back = render(BackLink {
        href: "/authz/roles",
        id: "role-back",
        title: "Back to roles",
        ..BackLink::default()
    })?;
    let mut html = render(Header {
        title: Html::text(&role.name),
        subtitle: role.code.as_deref().unwrap_or_default(),
        actions: back,
        ..Header::default()
    })?;
    let description = if role.description.is_empty() {
        "—"
    } else {
        &role.description
    };
    let facts = render(Facts {
        items: vec![
            Fact {
                title: "Scope",
                value: render(scope_badge(role))?,
                ..Fact::default()
            },
            Fact {
                title: "Description",
                value: Html::text(description),
                ..Fact::default()
            },
            Fact {
                title: "Owning company",
                value: Html::text("—"),
                ..Fact::default()
            },
            Fact {
                title: "Grants all capabilities",
                value: Html::text(if role.grant_all { "Yes" } else { "No" }),
                ..Fact::default()
            },
        ],
        ..Facts::default()
    })?;
    html.push(&section(
        "role-details-heading",
        "role-summary",
        "Role Details",
        facts,
    )?);
    html.push(&section(
        "role-capabilities-heading",
        "role-capabilities-card",
        "Capabilities",
        render(CapabilityList {
            grant_all: role.grant_all,
            capabilities: &details.capabilities,
        })?,
    )?);
    let mut principals = Table::new(
        "role-principals",
        vec![
            Column::new("Type"),
            Column::new("Principal"),
            Column::new("Company"),
        ],
    );
    principals.caption = "Principals";
    for user in &details.principals {
        principals.rows.push(Row {
            id: format!("principal-role-{user}"),
            cells: vec![
                render(Badge::status(BadgeKind::Neutral, "User"))?,
                render(TextCell {
                    class: "tabular-nums text-ink-muted",
                    text: &user.to_string(),
                })?,
                render(TextCell {
                    class: "tabular-nums text-ink-muted",
                    text: "—",
                })?,
            ],
            actions: Html::EMPTY,
        });
    }
    if details.principals.is_empty() {
        principals.empty = Some(TableEmpty::Content(Html::text(
            "Nobody in this tenant holds this role.",
        )));
    }
    html.push(&section(
        "role-principals-heading",
        "role-principals-card",
        "Principals",
        render(principals)?,
    )?);
    Ok(render(bilibi_ui::Page {
        variant: PageVariant::Detail,
        body: html,
        ..bilibi_ui::Page::default()
    })?
    .as_str()
    .to_owned())
}

#[derive(Template)]
#[template(
    source = r#"<form id="role-form" data-event="create-role" class="space-y-5"><section class="rounded-xl border border-line bg-surface px-6 py-5">{{ name }}{{ code }}{{ description }}</section><div class="flex items-center justify-end gap-3"><a id="role-cancel" href="/authz/roles" class="text-sm font-medium text-ink-muted hover:text-ink">Cancel</a>{{ save }}</div></form>"#,
    ext = "html"
)]
struct RoleForm {
    name: Html,
    code: Html,
    description: Html,
    save: Html,
}

fn render_create(role: &NewRole, errors: &RoleErrors, created: bool) -> Result<String> {
    let field = |key: &str, label: &str, value: &str, errors: &[String]| {
        Field::in_form("role", key)
            .label(label)
            .value(value)
            .errors(errors.to_vec())
    };
    let mut html = render(FlashGroup {
        id: "flash-group",
        flash: FlashMap {
            success: if created { "Role created." } else { "" },
            ..FlashMap::default()
        },
    })?;
    html.push(&render(Header {
        title: Html::text("Create Role"),
        subtitle: "A custom role owned by this tenant",
        actions: render(BackLink {
            href: "/authz/roles",
            id: "role-back",
            title: "Back to roles",
            ..BackLink::default()
        })?,
        ..Header::default()
    })?);
    html.push(&render(RoleForm {
        name: render(field("name", "Name", &role.name, &errors.name))?,
        code: render(
            field("code", "Code", &role.code, &errors.code)
                .placeholder("billing_manager")
                .hint("Lowercase letters, digits and underscores. Unique within this tenant."),
        )?,
        description: render(
            field(
                "description",
                "Description",
                &role.description,
                &errors.description,
            )
            .kind(InputKind::Textarea),
        )?,
        save: render(Button {
            variant: ButtonVariant::Primary,
            id: "role-save",
            button_type: "submit",
            working: "Creating...",
            body: Html::text("Create Role"),
            ..Button::default()
        })?,
    })?);
    Ok(render(bilibi_ui::Page {
        id: "role-create-page",
        variant: PageVariant::Form,
        body: html,
        ..bilibi_ui::Page::default()
    })?
    .as_str()
    .to_owned())
}

pub(crate) fn create_view() -> Result<String> {
    render_create(&NewRole::default(), &RoleErrors::default(), false)
}

/// Mounted list state: the URL carries the whole view, so a filtered, sorted
/// page is a link someone can send, and Back works.
pub(crate) struct Roles {
    pub(crate) pages: Pages,
    pub(crate) state: ListState,
}

impl LiveState<Scope, Error> for Roles {
    fn update<'a>(&'a mut self, request: LiveRequest<'a, Scope>) -> LiveFuture<'a, Error> {
        Box::pin(async move {
            match request.input {
                LiveInput::Refresh => {}
                LiveInput::Params(values) => self.state = parse(values)?,
                LiveInput::Event(value) => {
                    let get = |key: &str| value.get(key).and_then(|v| v.as_str());
                    match get("type") {
                        Some("search") => self
                            .state
                            .apply_filters(&params::form_values(value, FILTERS_FORM)),
                        Some("sort") => {
                            if !self.state.next_sort(get("sort").unwrap_or_default()) {
                                return Err(Error::Invalid);
                            }
                        }
                        Some("page") => self.state.put_page(get("page")),
                        _ => return Err(Error::Invalid),
                    }
                }
            }
            let html = self
                .pages
                .roles_view(request.scope, &mut self.state)
                .await?;
            Ok(LiveBody {
                html,
                query: self.state.to_params(),
                ..LiveBody::default()
            })
        })
    }
}

/// Mounted detail state; a refresh re-reads the role, so a grant change made
/// elsewhere shows on the next update.
pub(crate) struct RoleShow {
    pub(crate) pages: Pages,
    pub(crate) id: i64,
}

impl LiveState<Scope, Error> for RoleShow {
    fn update<'a>(&'a mut self, request: LiveRequest<'a, Scope>) -> LiveFuture<'a, Error> {
        Box::pin(async move {
            match request.input {
                LiveInput::Refresh => {}
                LiveInput::Params(_) | LiveInput::Event(_) => return Err(Error::Invalid),
            }
            Ok(LiveBody {
                html: self.pages.role_view(request.scope, self.id).await?,
                ..LiveBody::default()
            })
        })
    }
}

#[derive(serde::Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
enum CreateEvent {
    CreateRole {
        #[serde(rename = "role[name]", default)]
        name: String,
        #[serde(rename = "role[code]", default)]
        code: String,
        #[serde(rename = "role[description]", default)]
        description: String,
    },
    ClearFlash {
        key: String,
    },
}

/// Mounted create form. The form keeps what was typed while it is refused;
/// a created role lands on its detail page, which is where its capabilities
/// are read, or on the list, or back here with a cleared form for an actor
/// who may open neither.
pub(crate) struct RoleCreate {
    pub(crate) pages: Pages,
    pub(crate) role: NewRole,
    pub(crate) errors: RoleErrors,
    pub(crate) created: bool,
}

impl LiveState<Scope, Error> for RoleCreate {
    fn update<'a>(&'a mut self, request: LiveRequest<'a, Scope>) -> LiveFuture<'a, Error> {
        Box::pin(async move {
            match request.input {
                LiveInput::Refresh => {}
                LiveInput::Params(_) => return Err(Error::Invalid),
                LiveInput::Event(value) => {
                    match serde_json::from_value::<CreateEvent>(value.clone())
                        .map_err(|_| Error::Invalid)?
                    {
                        CreateEvent::CreateRole {
                            name,
                            code,
                            description,
                        } => {
                            self.created = false;
                            self.role = NewRole {
                                name: name.trim().to_owned(),
                                code: code.trim().to_owned(),
                                description: description.trim().to_owned(),
                            };
                            match self
                                .pages
                                .grants
                                .create_role(request.scope, &self.role)
                                .await?
                            {
                                CreatedRole::Refused(errors) => self.errors = errors,
                                CreatedRole::Created(id) => {
                                    let client = self.pages.db.get().await?;
                                    for (key, to) in [
                                        (roles::VIEW, format!("/authz/roles/{id}")),
                                        (roles::LIST, "/authz/roles".to_owned()),
                                    ] {
                                        if crate::decide(&**client, request.scope, key)
                                            .await?
                                            .allowed
                                        {
                                            return Ok(LiveBody {
                                                redirect: Some(to),
                                                ..LiveBody::default()
                                            });
                                        }
                                    }
                                    self.role = NewRole::default();
                                    self.errors = RoleErrors::default();
                                    self.created = true;
                                }
                            }
                        }
                        CreateEvent::ClearFlash { key } => {
                            if key == "success" {
                                self.created = false;
                            }
                        }
                    }
                }
            }
            Ok(LiveBody {
                html: render_create(&self.role, &self.errors, self.created)?,
                ..LiveBody::default()
            })
        })
    }
}
