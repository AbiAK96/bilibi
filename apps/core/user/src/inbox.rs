//! `/notifications` (Bilimbi `NotificationsLive`) and the shell's bell
//! (`NotificationBellComponent`). Both read and write only the sealed actor's
//! own notifications through `crate::notifications`, and both subscribe to
//! that account's committed changes: a notification arriving for someone else
//! never wakes them, and the bell's wake re-renders the bell alone, so a draft
//! on the page underneath is never touched by a notification.
use crate::{
    notifications::{self, Item, Status},
    web::Pages,
    UserService,
};
use askama::Template;
use bilibi_database::{Error, Result};
use bilibi_datetime::{DisplayContext, Format, Precision};
use bilibi_security::Scope;
use bilibi_ui::{
    list_state::FILTERS_FORM,
    page::{LiveBody, LiveFuture, LiveInput, LiveRequest, LiveState, PageBody, Subscription},
    params, Button, Filter, FlashMap, Header, Html, Icon, ListSpec, ListState, PageCounts,
    Pagination,
};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
    time::SystemTime,
};
use tokio_postgres::GenericClient;

/// The bell lists this many of the newest notifications.
const RECENT: i64 = 5;
const PAGE_SIZES: [u64; 4] = [25, 50, 100, 300];
const ALL_READ: &str = "All notifications marked as read.";

/// Bilimbi's query names: `filter`, `page`, `per_page`.
static SPEC: LazyLock<Arc<ListSpec>> = LazyLock::new(|| {
    ListSpec {
        page_sizes: PAGE_SIZES.to_vec(),
        page_size_param: "per_page".into(),
        filters: [("filter".into(), Filter::one_of(&Status::KEYS, "all"))].into(),
        omit_blank: vec!["search".into()],
        ..ListSpec::default()
    }
    .shared()
    .expect("notifications list spec")
});

fn fragment(rendered: askama::Result<Html>) -> Result<Html> {
    rendered.map_err(|_| Error::Internal)
}

/// A stored id travels as a button value; anything else is a bad intent.
fn notification_id(value: &str) -> Result<i64> {
    params::positive_id(Some(value)).ok_or(Error::Invalid)
}

// ---- Bell -------------------------------------------------------------------

#[derive(Template)]
#[template(path = "bell.html")]
struct BellView {
    unread: i64,
    badge: String,
    aria_label: String,
    items: Vec<Item>,
}

async fn bell_html(client: &impl GenericClient, scope: &Scope) -> Result<String> {
    let unread = notifications::unread(client, scope).await?;
    let items = notifications::list(client, scope, Status::All, RECENT, 0).await?;
    let badge = if unread > 99 {
        "99+".to_owned()
    } else {
        unread.to_string()
    };
    let aria_label = match unread {
        0 => "Notifications".to_owned(),
        1 => "1 unread notification".to_owned(),
        _ => format!("{badge} unread notifications"),
    };
    BellView {
        unread,
        badge,
        aria_label,
        items,
    }
    .render()
    .map_err(|_| Error::Internal)
}

pub(crate) async fn render_bell(pages: &Pages, scope: &Scope) -> Result<PageBody> {
    let client = pages.users.db.get().await?;
    Ok(PageBody {
        html: bell_html(&**client, scope).await?,
        live: true,
        title: None,
    })
}

pub(crate) fn mount_bell(pages: &Pages) -> Box<dyn LiveState<Scope, Error>> {
    Box::new(Bell {
        users: pages.users.clone(),
    })
}

struct Bell {
    users: Arc<UserService>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum BellEvent {
    MarkAllRead,
    Visit { id: String },
}

impl LiveState<Scope, Error> for Bell {
    fn update<'a>(&'a mut self, request: LiveRequest<'a, Scope>) -> LiveFuture<'a, Error> {
        Box::pin(async move {
            let mut redirect = None;
            match request.input {
                LiveInput::Refresh => {}
                LiveInput::Params(_) => return Err(Error::Invalid),
                LiveInput::Event(value) => {
                    match BellEvent::deserialize(value).map_err(|_| Error::Invalid)? {
                        BellEvent::MarkAllRead => {
                            self.users.mark_all_read(request.credential).await?;
                        }
                        BellEvent::Visit { id } => {
                            let item = self
                                .users
                                .mark_read(request.credential, notification_id(&id)?)
                                .await?;
                            redirect = item.url;
                        }
                    }
                }
            }
            let client = self.users.db.get().await?;
            Ok(LiveBody {
                html: bell_html(&**client, request.scope).await?,
                redirect,
                ..LiveBody::default()
            })
        })
    }
    fn subscriptions(&mut self) -> Vec<Subscription> {
        vec![self.users.notifications.subscribe()]
    }
}

// ---- /notifications -----------------------------------------------------------

/// One rendered instant (Bilimbi `<.datetime>`): both server-decidable texts
/// travel with the element so the shell's display-mode control can swap them.
struct Instant {
    id: String,
    iso: String,
    text: String,
    company: String,
    utc: String,
    mode: &'static str,
}
fn instant(display: &DisplayContext, id: String, at: SystemTime) -> Result<Instant> {
    let timestamp = jiff::Timestamp::try_from(at).map_err(|_| Error::Internal)?;
    let rendered = display.render(timestamp, Format::DateTime, Precision::Minute);
    Ok(Instant {
        id,
        iso: rendered.iso,
        text: rendered.text,
        company: rendered.company,
        utc: rendered.utc,
        mode: rendered.mode.as_str(),
    })
}

struct Row {
    id: String,
    mark_read_id: String,
    icon: String,
    created: Instant,
    item: Item,
}

#[derive(Template)]
#[template(path = "notifications.html")]
struct InboxView<'a> {
    flash: FlashMap<'a>,
    header: Html,
    filter: &'static str,
    per_page: u64,
    unread: i64,
    items: Vec<Row>,
    pagination: Html,
}
impl InboxView<'_> {
    fn tab_href(&self, filter: &str) -> String {
        format!(
            "/notifications?filter={filter}&page=1&per_page={}",
            self.per_page
        )
    }
}

async fn render_inbox(
    pages: &Pages,
    scope: &Scope,
    list: &mut ListState,
    flash: FlashMap<'_>,
) -> Result<String> {
    let client = pages.users.db.get().await?;
    let client = &**client;
    let status = Status::parse(list.filters.get("filter").map_or("all", String::as_str));
    let total = u64::try_from(notifications::count(client, scope, status).await?)
        .map_err(|_| Error::Internal)?;
    // A read row can leave the Unread filter's last page empty; show the new last page.
    list.clamp_to_last_page(total.div_ceil(list.page_size), true);
    let unread = notifications::unread(client, scope).await?;
    let limit = i64::try_from(list.page_size).map_err(|_| Error::Invalid)?;
    let offset = i64::try_from(list.offset()).map_err(|_| Error::Invalid)?;
    let found = notifications::list(client, scope, status, limit, offset).await?;
    let locale = pages.locales.resolve(client, scope, None).await?.locale;
    let display = pages.display.display(client, scope, None, locale).await?;
    let items = found
        .into_iter()
        .map(|item| {
            let icon = item
                .icon
                .clone()
                .filter(|name| bilibi_ui::icons::renderable(name))
                .unwrap_or_else(|| "notify".into());
            Ok(Row {
                id: item.id.to_string(),
                mark_read_id: format!("mark-read-{}", item.id),
                icon,
                created: instant(
                    &display,
                    format!("notification-created-{}", item.id),
                    item.created_at,
                )?,
                item,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let actions = if unread > 0 {
        let mut body = fragment(Html::render(&Icon {
            name: "confirm",
            class: "size-3.5 mr-1",
        }))?;
        body.push(&Html::text("Mark all as read"));
        fragment(Html::render(&Button {
            id: "mark-all-read-btn",
            button_type: "button",
            event: "mark_all_read",
            class: "text-xs px-3 py-1.5",
            body,
            ..Button::default()
        }))?
    } else {
        Html::EMPTY
    };
    let header = fragment(Html::render(&Header {
        title: Html::text("Notifications"),
        subtitle: "Updates, system events, and account activity",
        title_actions: Html::EMPTY,
        actions,
    }))?;
    let pagination = fragment(Html::render(&Pagination::new(
        "notifications-pagination",
        PageCounts::new(list.page, list.page_size, total),
        &PAGE_SIZES,
    )))?;
    InboxView {
        flash,
        header,
        filter: status.key(),
        per_page: list.page_size,
        unread,
        items,
        pagination,
    }
    .render()
    .map_err(|_| Error::Internal)
}

fn parse(query: &HashMap<String, String>) -> Result<ListState> {
    ListState::parse(query, SPEC.clone()).map_err(|_| Error::Internal)
}

pub(crate) async fn render_page(
    pages: &Pages,
    scope: &Scope,
    query: &HashMap<String, String>,
) -> Result<PageBody> {
    let mut list = parse(query)?;
    Ok(PageBody {
        html: render_inbox(pages, scope, &mut list, FlashMap::default()).await?,
        live: true,
        title: None,
    })
}

pub(crate) fn mount_page(
    pages: &Pages,
    query: &HashMap<String, String>,
) -> Result<Box<dyn LiveState<Scope, Error>>> {
    Ok(Box::new(Inbox {
        users: pages.users.clone(),
        display: pages.display.clone(),
        locales: pages.locales.clone(),
        list: parse(query)?,
        all_read: false,
    }))
}

struct Inbox {
    users: Arc<UserService>,
    display: Arc<bilibi_datetime::DisplayPolicy>,
    locales: Arc<bilibi_locale::Locales>,
    list: ListState,
    /// The "all marked as read" flash, shown until dismissed.
    all_read: bool,
}

impl Inbox {
    /// Bilimbi's events; the pagination posts `filters` (its rows-per-page
    /// form) and `page`.
    async fn event(&mut self, credential: &str, value: &Value) -> Result<Option<String>> {
        let get = |key: &str| value.get(key).and_then(Value::as_str);
        match get("type").ok_or(Error::Invalid)? {
            "mark_read" => {
                let id = notification_id(get("id").ok_or(Error::Invalid)?)?;
                self.users.mark_read(credential, id).await?;
            }
            "visit" => {
                let id = notification_id(get("id").ok_or(Error::Invalid)?)?;
                return Ok(self.users.mark_read(credential, id).await?.url);
            }
            "mark_all_read" => {
                self.users.mark_all_read(credential).await?;
                self.all_read = true;
            }
            "filters" => self
                .list
                .apply_filters(&params::form_values(value, FILTERS_FORM)),
            "page" => self.list.put_page(get("page")),
            "clear-flash" => self.all_read = false,
            _ => return Err(Error::Invalid),
        }
        Ok(None)
    }
}

impl LiveState<Scope, Error> for Inbox {
    fn update<'a>(&'a mut self, request: LiveRequest<'a, Scope>) -> LiveFuture<'a, Error> {
        Box::pin(async move {
            let notice_before = self.all_read;
            let mut redirect = None;
            match request.input {
                LiveInput::Refresh => {}
                LiveInput::Params(values) => self.list = parse(values)?,
                LiveInput::Event(value) => redirect = self.event(request.credential, value).await?,
            }
            let flash = FlashMap {
                success: if self.all_read { ALL_READ } else { "" },
                ..FlashMap::default()
            };
            let pages = Pages {
                users: self.users.clone(),
                display: self.display.clone(),
                locales: self.locales.clone(),
            };
            let html = render_inbox(&pages, request.scope, &mut self.list, flash).await?;
            Ok(LiveBody {
                html,
                query: self.list.to_params(),
                notice: if self.all_read && !notice_before {
                    ALL_READ.into()
                } else {
                    String::new()
                },
                redirect,
                ..LiveBody::default()
            })
        })
    }
    fn subscriptions(&mut self) -> Vec<Subscription> {
        vec![self.users.notifications.subscribe()]
    }
}
