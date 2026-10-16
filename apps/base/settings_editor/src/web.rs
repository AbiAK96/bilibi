//! `/system/settings`, the operator settings page (Bilimbi
//! `Settings.Web.GroupLive`). The screen owns no rules: `form` decides what a
//! field is, whether it is inherited, what clearing means and what a secret may
//! show; this renders that and hands submissions back.
//!
//! Every write re-asks Authz (`authorize_operator` on the route capability)
//! and rebuilds the field plan from live grants before it acts, so a field
//! whose capability was revoked since the page drew is ignored even when a
//! forged submission names it. Refusals flash and redraw; they never clear
//! the page. Restore confirms through the shared dialog and acts only on the
//! overrides the dialog counted.
//!
//! The company picker lists no companies yet: Core binds company targets
//! (port slice K15). Until then an actor granted company settings sees the
//! picker with Global only and is told no company is available to manage.
use crate::{
    form::{self, Field, Plan, Step},
    reveal::{self, Refusal, Reveal, Throttle},
    COMPANY_MANAGE, GLOBAL_MANAGE,
};
use askama::Template;
use bilibi_authz::authorize_operator;
use bilibi_database::{Db, Error, MutationBoundary, Result};
use bilibi_settings::{Settings, Target, ValueType};
use bilibi_tenancy::{Authentication, Scope};
use bilibi_ui::{
    choices,
    forms::{Field as Control, InputKind},
    page::{
        LiveBody, LiveFuture, LiveInput, LiveRequest, LiveState, MountFuture, Page, PageBody,
        PageFuture, PageRequest,
    },
    Attrs, Button, ButtonVariant, ConfirmDialog, EmptyState, FlashMap, Header, Html, IconButton,
    IconContext, SecretInput, StatusKind,
};
use serde_json::{json, Value};
use std::sync::Arc;

const TITLE: &str = "Operator Settings";
const PLATFORM_DENIED: &str = "You do not have permission to manage platform settings.";
const COMPANY_DENIED: &str = "You do not have permission to manage this company's settings.";
const UNAVAILABLE: &str = "Settings are unavailable; nothing was changed.";

pub struct Core {
    pub db: Db,
    pub settings: Arc<Settings>,
    pub boundary: Arc<MutationBoundary>,
    pub authentication: Arc<Authentication>,
    pub throttle: Throttle,
}
pub struct Pages(Arc<Core>);
impl Pages {
    pub fn new(core: Core) -> Self {
        Self(Arc::new(core))
    }
}

struct Editor {
    core: Arc<Core>,
    flash: Option<(StatusKind, String)>,
    /// The overrides the restore dialog counted; restore acts on these only.
    pending_restore: Option<Vec<String>>,
    pending_reveal: Option<String>,
    events: Vec<(&'static str, Value)>,
}

impl Editor {
    fn new(core: Arc<Core>) -> Self {
        Self {
            core,
            flash: None,
            pending_restore: None,
            pending_reveal: None,
            events: Vec::new(),
        }
    }
    fn flash(&mut self, kind: StatusKind, message: impl Into<String>) {
        self.flash = Some((kind, message.into()));
    }
    async fn plan(&self, scope: &Scope) -> Result<Plan> {
        let client = self.core.db.get().await?;
        form::plan(&**client, &self.core.settings, scope).await
    }
    /// The route capability, asked again (and logged) before any write.
    async fn writable(&mut self, scope: &Scope) -> Result<bool> {
        let client = self.core.db.get().await?;
        match authorize_operator(&**client, scope, GLOBAL_MANAGE).await {
            Ok(_) => Ok(true),
            Err(Error::Forbidden) => {
                self.flash(StatusKind::Error, PLATFORM_DENIED);
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }

    async fn event(&mut self, scope: &Scope, payload: &Value) -> Result<()> {
        let get = |key: &str| payload.get(key).and_then(Value::as_str);
        match get("type").ok_or(Error::Invalid)? {
            "save_settings" => self.save(scope, payload).await?,
            "request_restore" => {
                let plan = self.plan(scope).await?;
                let overridden: Vec<String> = plan
                    .fields
                    .iter()
                    .filter(|field| field.overridden)
                    .map(|field| field.key.to_owned())
                    .collect();
                if overridden.is_empty() {
                    self.flash(StatusKind::Info, form::restored_message(0));
                } else {
                    self.flash = None;
                    self.pending_restore = Some(overridden);
                }
            }
            "cancel_restore" => self.pending_restore = None,
            "restore_defaults" => self.restore(scope).await?,
            "switch_scope" => match get("scope[company_id]") {
                Some("global") => {
                    self.flash = None;
                    self.pending_restore = None;
                    self.pending_reveal = None;
                }
                Some(_) => self.flash(StatusKind::Error, COMPANY_DENIED),
                None => self.flash(StatusKind::Error, "Choose an available settings scope."),
            },
            "request_secret_reveal" => {
                let key = get("key").unwrap_or_default();
                let plan = self.plan(scope).await?;
                let client = self.core.db.get().await?;
                let offered = plan
                    .field(key)
                    .is_some_and(|field| field.encrypted() && field.stored_secret);
                if offered
                    && reveal::available(&**client, &plan, &self.core.settings, scope).await?
                {
                    self.pending_reveal = Some(key.to_owned());
                } else {
                    // Even a forged request is recorded as a refused reveal.
                    reveal::refuse(&**client, scope, key).await?;
                    self.flash(StatusKind::Error, "Stored value cannot be shown.");
                }
            }
            "cancel_secret_reveal" => self.pending_reveal = None,
            "confirm_secret_reveal" => self.confirm_reveal(scope, payload).await?,
            _ => return Err(Error::Invalid),
        }
        Ok(())
    }

    async fn save(&mut self, scope: &Scope, payload: &Value) -> Result<()> {
        if !self.writable(scope).await? {
            return Ok(());
        }
        // Planned in full before anything is written, so a bad later field
        // cannot leave earlier ones committed by a save that reported failure.
        let plan = self.plan(scope).await?;
        let mut steps = Vec::new();
        for field in &plan.fields {
            let Some(submitted) = payload
                .get(format!("settings[{}]", field.key))
                .and_then(Value::as_str)
            else {
                continue;
            };
            match form::decide(field, submitted) {
                Ok(Step::Skip) => {}
                Ok(Step::Write(_))
                    if field.encrypted() && !self.core.settings.encryption_available() =>
                {
                    self.flash(
                        StatusKind::Error,
                        format!(
                            "{}: cannot be changed until the settings encryption key is configured",
                            field.editor.label
                        ),
                    );
                    return Ok(());
                }
                Ok(step) => steps.push((field, step)),
                Err(message) => {
                    self.flash(
                        StatusKind::Error,
                        format!("{}: {message}", field.editor.label),
                    );
                    return Ok(());
                }
            }
        }
        let (mut written, mut cleared) = (0, 0);
        if !steps.is_empty() {
            let mut client = self.core.db.get().await?;
            let mut tx = self
                .core
                .boundary
                .begin(&mut client, scope.mutation_context("settings.global.save"))
                .await?;
            for (field, step) in steps {
                let result = match step {
                    Step::Write(value) => {
                        written += 1;
                        self.core
                            .settings
                            .put(&mut tx, scope, Target::Global, field.key, value)
                            .await
                    }
                    Step::Clear => {
                        cleared += 1;
                        self.core
                            .settings
                            .delete(&mut tx, scope, Target::Global, field.key)
                            .await
                    }
                    Step::Skip => Ok(()),
                };
                if let Err(error) = result {
                    // Dropping the transaction rolls every step back.
                    return self.refused(error, Some(field.editor.label));
                }
            }
            if let Err(error) = tx.commit().await {
                return self.refused(error, None);
            }
        }
        let kind = if written + cleared == 0 {
            StatusKind::Info
        } else {
            StatusKind::Success
        };
        self.flash(kind, form::saved_message(written, cleared));
        Ok(())
    }

    async fn restore(&mut self, scope: &Scope) -> Result<()> {
        let Some(held) = self.pending_restore.take() else {
            return Ok(());
        };
        if !self.writable(scope).await? {
            return Ok(());
        }
        // Asked now, not read off the page: a save since the dialog opened may
        // already have cleared one, and a revoked field is no longer shown.
        let plan = self.plan(scope).await?;
        let fields: Vec<&Field> = plan
            .fields
            .iter()
            .filter(|field| field.overridden && held.iter().any(|key| key == field.key))
            .collect();
        if !fields.is_empty() {
            let mut client = self.core.db.get().await?;
            let mut tx = self
                .core
                .boundary
                .begin(
                    &mut client,
                    scope.mutation_context("settings.global.restore"),
                )
                .await?;
            for field in &fields {
                if let Err(error) = self
                    .core
                    .settings
                    .delete(&mut tx, scope, Target::Global, field.key)
                    .await
                {
                    return self.refused(error, Some(field.editor.label));
                }
            }
            if let Err(error) = tx.commit().await {
                return self.refused(error, None);
            }
        }
        let kind = if fields.is_empty() {
            StatusKind::Info
        } else {
            StatusKind::Success
        };
        self.flash(kind, form::restored_message(fields.len()));
        Ok(())
    }

    /// A refused or failed write is a flash, never a page-clearing error; only
    /// a lost session propagates.
    fn refused(&mut self, error: Error, label: Option<&str>) -> Result<()> {
        let message = match error {
            Error::Unauthorized => return Err(Error::Unauthorized),
            Error::Forbidden => PLATFORM_DENIED.to_owned(),
            Error::Invalid => match label {
                Some(label) => format!("{label}: the value was refused; nothing was changed."),
                None => UNAVAILABLE.to_owned(),
            },
            _ => UNAVAILABLE.to_owned(),
        };
        self.flash(StatusKind::Error, message);
        Ok(())
    }

    async fn confirm_reveal(&mut self, scope: &Scope, payload: &Value) -> Result<()> {
        let Some(key) = self.pending_reveal.clone() else {
            return Ok(());
        };
        let password = payload
            .get("reveal[password]")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let plan = self.plan(scope).await?;
        let client = self.core.db.get().await?;
        let reveal = Reveal {
            settings: &self.core.settings,
            authentication: &self.core.authentication,
            throttle: &self.core.throttle,
        };
        match reveal.reveal(&**client, &plan, scope, &key, password).await {
            Ok(value) => {
                let duration = plan
                    .field(&key)
                    .map_or(10_000, |field| field.editor.reveal_duration_ms);
                self.pending_reveal = None;
                self.flash = None;
                self.events.push((
                    "secret:reveal",
                    json!({"id": format!("input-{}", form::dom(&key)), "value": value, "duration_ms": duration}),
                ));
            }
            Err(Refusal::Throttled) => {
                self.flash(StatusKind::Error, "Too many attempts. Try again later.")
            }
            Err(Refusal::InvalidPassword) => {
                self.flash(StatusKind::Error, "Password was not accepted.")
            }
            Err(Refusal::AuditUnavailable) => self.flash(
                StatusKind::Error,
                "The reveal could not be recorded, so the value was not shown. Try again later.",
            ),
            Err(Refusal::Refused) => {
                self.pending_reveal = None;
                self.flash(StatusKind::Error, "This stored value cannot be shown.");
            }
        }
        Ok(())
    }

    async fn render(&self, scope: &Scope) -> Result<String> {
        let plan = self.plan(scope).await?;
        let client = self.core.db.get().await?;
        let can_reveal = reveal::available(&**client, &plan, &self.core.settings, scope).await?;
        let keyed = self.core.settings.encryption_available();
        let fields = plan
            .fields
            .iter()
            .map(|field| card(field, can_reveal, keyed))
            .collect::<Result<Vec<_>>>()?;
        let empty = if fields.is_empty() {
            let state = if plan.withheld.is_empty() {
                EmptyState::missing(
                    "No installed module contributes settings to this group.",
                    "",
                )
                .id("settings-group-operator-empty")
            } else {
                EmptyState::forbidden(withheld_wording(&plan.withheld))
                    .id("settings-group-operator-withheld")
            };
            render(EmptyCard { state })?
        } else {
            Html::EMPTY
        };
        let header = render(Header {
            title: Html::text(TITLE),
            subtitle: "Settings contributed by installed modules.",
            title_actions: render(IconButton {
                context: IconContext::Inline,
                id: "settings-pin",
                nav_pin: "nav-admin-system-settings",
                ..IconButton::new("bilimbi-pin", "Pin Operator Settings to sidebar")
            })?,
            ..Header::default()
        })?;
        let scopes = if plan.capabilities.contains(COMPANY_MANAGE) {
            let picker = Control::named("scope[company_id]")
                .id("settings-company")
                .kind(InputKind::Select)
                .label("Settings scope")
                .options(choices(&[("Global", "global")]))
                .value("global");
            render(ScopePicker {
                picker,
                none: EmptyState::missing(
                    "No companies are available to manage.",
                    "Ask an operator to review your company access.",
                )
                .id("company-settings-no-targets"),
            })?
        } else {
            render(Withheld {
                state: EmptyState::forbidden("manage company settings")
                    .id("company-settings-withheld"),
            })?
        };
        let reveal = match &self.pending_reveal {
            Some(_) => render(RevealConfirm {
                password: SecretInput::new(
                    "secret-reveal-password",
                    "reveal[password]",
                    "password",
                )
                .label("Your password")
                .no_reveal()
                .attrs(Attrs {
                    autocomplete: Some("current-password".into()),
                    required: true,
                    ..Attrs::default()
                }),
            })?,
            None => Html::EMPTY,
        };
        let dialog = match &self.pending_restore {
            Some(held) => {
                let consequence = format!(
                    "{} on this page will be removed.",
                    form::count(held.len(), "override").unwrap_or_default()
                );
                render(ConfirmDialog {
                    id: "restore-defaults-confirm",
                    consequence: &consequence,
                    detail: "Each of those settings returns to the value it inherits. This cannot be undone.",
                    confirm: "Restore",
                    working: "Restoring…",
                    confirm_event: "restore_defaults",
                    cancel_event: "cancel_restore",
                })?
            }
            None => Html::EMPTY,
        };
        let message = |kind| match &self.flash {
            Some((k, text)) if *k == kind => text.as_str(),
            _ => "",
        };
        let view = SettingsView {
            flash: FlashMap {
                error: message(StatusKind::Error),
                warning: message(StatusKind::Warning),
                success: message(StatusKind::Success),
                info: message(StatusKind::Info),
            },
            header,
            scopes,
            fields,
            empty,
            restore: render(Button {
                id: "settings-restore",
                button_type: "button",
                event: "request_restore",
                body: Html::text("Restore Defaults"),
                ..Button::default()
            })?,
            save: render(Button {
                variant: ButtonVariant::Primary,
                id: "settings-save",
                button_type: "submit",
                body: Html::text("Save"),
                ..Button::default()
            })?,
            reveal,
            dialog,
        };
        view.render().map_err(|_| Error::Internal)
    }
}

/// The action `EmptyState::forbidden` completes into "You do not have
/// permission to ...", naming each capability the actor lacks.
fn withheld_wording(capabilities: &std::collections::BTreeSet<&str>) -> String {
    let names: Vec<&str> = capabilities.iter().copied().collect();
    match names.as_slice() {
        [one] => format!("see the settings in this group, each of which needs {one}"),
        [rest @ .., last] => format!(
            "see the settings in this group; each setting needs its own permission, and this group uses {} and {last}",
            rest.join(", ")
        ),
        [] => String::new(),
    }
}

fn card(field: &Field, can_reveal: bool, keyed: bool) -> Result<FieldCard> {
    let dom = field.dom();
    let id = format!("input-{dom}");
    let name = format!("settings[{}]", field.key);
    let control = if field.definition.value_type == ValueType::Boolean {
        render(
            Control::named(name)
                .id(id)
                .kind(InputKind::Checkbox)
                .checked(field.value == Value::Bool(true))
                .label("Enabled")
                .wrapper_class("mb-0 mt-2.5"),
        )?
    } else if field.encrypted() {
        let mut secret = SecretInput::new(id, name, field.editor.label)
            .autocomplete("off")
            .wrapper_class("mt-2.5");
        secret.class = "w-full rounded-md border border-line bg-surface px-3 py-1.5 text-sm text-ink shadow-xs transition placeholder:text-ink-faint focus:border-brand-strong focus:outline-none focus:ring-2 focus:ring-brand-strong/30".into();
        if field.stored_secret {
            secret = secret.stored();
            if can_reveal {
                secret = secret.stored_reveal("request_secret_reveal", field.key);
            }
        }
        if !keyed {
            secret = secret.readonly();
        }
        render(secret)?
    } else {
        render(PlainInput {
            kind: match field.definition.value_type {
                ValueType::Integer | ValueType::Float => "number",
                _ => "text",
            },
            float: field.definition.value_type == ValueType::Float,
            id: &id,
            name: &name,
            value: form::input_value(field),
        })?
    };
    Ok(FieldCard {
        dom,
        label: field.editor.label,
        help: field.editor.help,
        control,
        overridden: field.overridden,
        encrypted: if !field.encrypted() {
            ""
        } else if keyed {
            "Stored encrypted; leave the mask to keep the current value"
        } else {
            "Stored encrypted; read-only until the settings encryption key is configured"
        },
    })
}

struct FieldCard {
    dom: String,
    label: &'static str,
    help: &'static str,
    control: Html,
    overridden: bool,
    encrypted: &'static str,
}

#[derive(Template)]
#[template(path = "settings.html")]
struct SettingsView<'a> {
    flash: FlashMap<'a>,
    header: Html,
    scopes: Html,
    fields: Vec<FieldCard>,
    empty: Html,
    restore: Html,
    save: Html,
    reveal: Html,
    dialog: Html,
}
/// Bilimbi renders a bare input under the card's own label here, not the
/// labelled `input` component; `step="any"` lets a fractional value submit.
#[derive(Template)]
#[template(
    source = r#"<input type="{{ kind }}" id="{{ id }}" name="{{ name }}" value="{{ value }}"{% if float %} step="any"{% endif %} autocomplete="off" class="mt-2.5 w-full rounded-md border border-line bg-surface px-3 py-1.5 text-sm text-ink shadow-xs transition placeholder:text-ink-faint focus:border-brand-strong focus:outline-none focus:ring-2 focus:ring-brand-strong/30" />"#,
    ext = "html"
)]
struct PlainInput<'a> {
    kind: &'a str,
    float: bool,
    id: &'a str,
    name: &'a str,
    value: String,
}
#[derive(Template)]
#[template(
    source = r#"{% import "lists.html" as lists %}<div class="rounded-xl border border-line bg-surface px-4 py-8">{% call lists::empty_state(e=state) %}{% endcall %}</div>"#,
    ext = "html"
)]
struct EmptyCard {
    state: EmptyState,
}
#[derive(Template)]
#[template(
    source = r#"{% import "forms.html" as forms %}{% import "lists.html" as lists %}<form id="settings-scope-form" data-event="switch_scope" data-change class="mt-4">{% call forms::input(field=picker) %}{% endcall %}</form>{% call lists::empty_state(e=none) %}{% endcall %}"#,
    ext = "html"
)]
struct ScopePicker {
    picker: Control,
    none: EmptyState,
}
#[derive(Template)]
#[template(
    source = r#"{% import "lists.html" as lists %}{% call lists::empty_state(e=state) %}{% endcall %}"#,
    ext = "html"
)]
struct Withheld {
    state: EmptyState,
}
#[derive(Template)]
#[template(path = "settings_reveal.html")]
struct RevealConfirm {
    password: SecretInput,
}
fn render(fragment: impl Template) -> Result<Html> {
    Html::render(&fragment).map_err(|_| Error::Internal)
}

impl Page<Scope, Error> for Pages {
    fn title(&self, operation: &str) -> Option<&'static str> {
        (operation == "global").then_some("Operator Settings")
    }
    fn render<'a>(&'a self, request: PageRequest<'a, Scope>) -> PageFuture<'a, Error> {
        Box::pin(async move {
            if request.operation != "global" {
                return Err(Error::NotFound);
            }
            let scope = request.scope.ok_or(Error::Unauthorized)?;
            Ok(PageBody {
                html: Editor::new(self.0.clone()).render(scope).await?,
                live: true,
                title: None,
            })
        })
    }
    fn mount<'a>(&'a self, request: PageRequest<'a, Scope>) -> MountFuture<'a, Scope, Error> {
        Box::pin(async move {
            if request.operation != "global" {
                return Err(Error::NotFound);
            }
            let state: Box<dyn LiveState<Scope, Error>> = Box::new(Editor::new(self.0.clone()));
            Ok(Some(state))
        })
    }
}

impl LiveState<Scope, Error> for Editor {
    fn update<'a>(&'a mut self, request: LiveRequest<'a, Scope>) -> LiveFuture<'a, Error> {
        Box::pin(async move {
            let before = self.flash.clone();
            match request.input {
                LiveInput::Refresh | LiveInput::Params(_) => {}
                LiveInput::Event(value) => self.event(request.scope, value).await?,
            }
            let html = self.render(request.scope).await?;
            let notice = match &self.flash {
                Some((_, message)) if self.flash != before => message.clone(),
                _ => String::new(),
            };
            Ok(LiveBody {
                html,
                notice,
                events: std::mem::take(&mut self.events),
                ..LiveBody::default()
            })
        })
    }
}
