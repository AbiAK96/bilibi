use super::*;
use crate::{contributions::CREATE, model::kind_key, PersonKind, Search};
use bilibi_grid::{Outcome, PageColumns};
use bilibi_ui::{
    choices,
    list_state::{InvalidPageSize, ParamNames},
    lists::RecordLink,
    Badge, BadgeKind, BuiltinColumn, Card, CreateButton, Filter, FilterToolbar, Header, IconButton,
    IconContext, ListSpec, ListState, Pagination, SortDir, TableEmpty, ToolbarControl,
};
use serde_json::json;
use std::{collections::BTreeMap, sync::LazyLock};

const SIZES: [u64; 3] = [25, 50, 100];
static SPEC: LazyLock<Arc<ListSpec>> = LazyLock::new(|| {
    ListSpec {
        sortable: BTreeMap::from([
            ("name".into(), SortDir::Asc),
            ("kind".into(), SortDir::Asc),
            ("created_at".into(), SortDir::Desc),
        ]),
        default_sort: Some("name".into()),
        page_sizes: SIZES.to_vec(),
        default_page_size: 25,
        page_size_param: "per_page".into(),
        invalid_page_size: InvalidPageSize::Default,
        filters: BTreeMap::from([
            (
                "kind".into(),
                Filter::one_of(&["all", "human", "agent"], "all"),
            ),
            (
                "retired".into(),
                Filter::one_of(&["active", "all"], "active"),
            ),
        ]),
        param_names: ParamNames {
            sort_by: "sort".into(),
            sort_dir: "dir".into(),
            ..ParamNames::default()
        },
        omit_blank: vec!["search".into()],
        ..ListSpec::default()
    }
    .shared()
    .expect("person list spec")
});

pub(super) struct Index {
    pages: Pages,
    list: ListState,
    columns: PageColumns,
    framed: bool,
}
impl Index {
    pub async fn new(
        pages: Pages,
        scope: &Scope,
        query: &HashMap<String, String>,
        framed: bool,
    ) -> Result<Self> {
        let mut columns = PageColumns::mount(
            pages.views.clone(),
            "persons",
            vec![
                BuiltinColumn::new("name", "Name").sortable("name"),
                BuiltinColumn::new("kind", "Kind").sortable("kind"),
                BuiltinColumn::new("state", "Identity"),
            ],
        )
        .map_err(|_| Error::Internal)?;
        columns.from_params(scope, query).await?;
        Ok(Self {
            pages,
            list: ListState::parse(query, SPEC.clone()).map_err(|_| Error::Internal)?,
            columns,
            framed,
        })
    }
    async fn view(&mut self, scope: &Scope) -> Result<LiveBody> {
        self.list.search = self.list.search.trim().chars().take(200).collect();
        let search = |list: &ListState| Search {
            search: list.search.trim().chars().take(200).collect(),
            kind: match list.filters["kind"].as_str() {
                "human" => Some(PersonKind::Human),
                "agent" => Some(PersonKind::Agent),
                _ => None,
            },
            include_retired: list.filters["retired"] == "all",
            page: list.page as i64,
            page_size: list.page_size as i64,
            sort: list.sort_by.clone().unwrap_or_else(|| "name".into()),
            descending: list.sort_dir == Some(SortDir::Desc),
        };
        let mut page = self
            .pages
            .persons
            .search(scope, &search(&self.list))
            .await?;
        let total_pages = (page.total as u64).div_ceil(self.list.page_size);
        if self.list.clamp_to_last_page(total_pages, true) {
            page = self
                .pages
                .persons
                .search(scope, &search(&self.list))
                .await?;
        }
        self.columns
            .load(
                scope,
                &page
                    .entries
                    .iter()
                    .map(|p| json!(p.id.get()))
                    .collect::<Vec<_>>(),
                &self.pages.display(scope).await?,
            )
            .await?;
        let mut actions = Html::EMPTY;
        if self.pages.allowed(scope, CREATE).await? {
            actions.push(&render(CreateButton {
                noun: "Person",
                href: "/persons/new",
                id: "person-new",
                ..CreateButton::default()
            })?);
        }
        let mut pin = IconButton::new("bilimbi-pin", "Pin People to sidebar");
        pin.id = "persons-pin";
        pin.context = IconContext::Inline;
        pin.nav_pin = "nav-admin-person";
        let mut body = render(Header {
            title: Html::text("People"),
            subtitle: "People and personal contacts",
            title_actions: render(pin)?,
            actions,
        })?;
        let form = self.list.filters_form();
        body.push(&render(FilterToolbar::new(
            "persons-filters",
            vec![
                ToolbarControl::search(
                    "persons-search",
                    "search",
                    "Search people",
                    &form["search"],
                    "Search by name, email, or phone...",
                ),
                ToolbarControl::select(
                    "persons-kind",
                    "kind",
                    "Kind",
                    &form["kind"],
                    choices(&[
                        ("All kinds", "all"),
                        ("Human only", "human"),
                        ("Agent only", "agent"),
                    ]),
                ),
                ToolbarControl::select(
                    "persons-retired",
                    "retired",
                    "Identity",
                    &form["retired"],
                    choices(&[("Current identities", "active"), ("Include retired", "all")]),
                ),
            ],
        ))?);
        let mut rows = self.columns.rows.clone();
        for row in &mut rows {
            let Some(person) = page
                .entries
                .iter()
                .find(|p| p.id.get().to_string() == row.key)
            else {
                continue;
            };
            row.dom_id = Some(format!("person-{}", person.id.get()));
            row.slots.insert(
                "name".into(),
                render(RecordLink {
                    workspace: self.framed,
                    kind: "core/person",
                    record_id: &person.id.get().to_string(),
                    href: &format!("/persons/{}", person.id.get()),
                    class: "font-medium text-ink-strong hover:underline",
                    body: Html::text(&person.name),
                })?,
            );
            row.slots.insert(
                "kind".into(),
                render(Badge {
                    body: Html::text(kind_key(person.kind)),
                    ..Badge::default()
                })?,
            );
            row.slots.insert(
                "state".into(),
                render(Badge::status(
                    if person.retired_at.is_some() {
                        BadgeKind::Neutral
                    } else {
                        BadgeKind::Success
                    },
                    if person.retired_at.is_some() {
                        "retired"
                    } else {
                        "current"
                    },
                ))?,
            );
        }
        let mut table = self
            .columns
            .flex_table("persons", rows)
            .sorted(self.list.sort_by.as_deref(), self.list.sort_dir);
        table.caption = "People";
        table.framed = false;
        if page.entries.is_empty() {
            table.empty = Some(TableEmpty::Content(Html::text("No people found.")));
        }
        let mut card = render(table)?;
        card.push(&render(Pagination::new(
            "persons-pagination",
            bilibi_ui::PageCounts::new(self.list.page, self.list.page_size, page.total as u64),
            &SIZES,
        ))?);
        body.push(&render(Card {
            id: "persons-card",
            inner_class: "p-0",
            body: card,
            ..Card::default()
        })?);
        let html = render(bilibi_ui::Page {
            id: "persons-index",
            body,
            ..bilibi_ui::Page::default()
        })?
        .to_string();
        let mut query = self.list.to_params();
        query.extend(self.columns.params());
        Ok(LiveBody {
            html,
            query,
            rows: page.entries.len(),
            ..LiveBody::default()
        })
    }
}
impl LiveState<Scope, Error> for Index {
    fn update<'a>(&'a mut self, request: LiveRequest<'a, Scope>) -> LiveFuture<'a, Error> {
        Box::pin(async move {
            match request.input {
                LiveInput::Refresh => {}
                LiveInput::Params(query) => {
                    self.list =
                        ListState::parse(query, SPEC.clone()).map_err(|_| Error::Internal)?;
                    self.columns.from_params(request.scope, query).await?;
                }
                LiveInput::Event(value) => match event(value)? {
                    "filters" => {
                        let filters = bilibi_ui::params::form_values(
                            value,
                            bilibi_ui::list_state::FILTERS_FORM,
                        );
                        self.list.apply_filters(&filters);
                    }
                    "page" => {
                        self.list
                            .put_page(value.get("page").and_then(Value::as_str));
                    }
                    "sort" => {
                        self.list.next_sort(value["sort"].as_str().unwrap_or(""));
                    }
                    "grid" => {
                        if let Outcome::Sort(key) =
                            self.columns.handle(request.scope, &values(value)).await?
                        {
                            self.list.next_sort(&key);
                        }
                    }
                    _ => return Err(Error::Invalid),
                },
            }
            self.view(request.scope).await
        })
    }
}
