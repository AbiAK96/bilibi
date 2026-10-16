//! `/addresses/create` (Bilimbi `CreateLive`): label, phone and street
//! lines, the shared location fields with their cascade, and provenance. It
//! validates as values change and creates through `AddressService::create`,
//! which is the authority on every refusal.
use super::{
    location::{Ids, LocationDraft, LABELS},
    *,
};
use crate::{contributions::CREATE, model, AddressInput, FieldError, Location};
use bilibi_ui::{
    BackLink, Button, ButtonVariant, Choice, Field, Header, InputKind, RestrictedField,
};

const IDS: Ids = Ids {
    prefix: "address",
    country: "address-country",
    admin1: "address-admin1",
};

#[derive(Template)]
#[template(path = "address_create.html")]
struct View<'a> {
    flash: FlashMap<'a>,
    header: Html,
    contact: Vec<Field>,
    restricted: Vec<RestrictedField>,
    lines: Vec<Field>,
    location: Html,
    provenance: Vec<Field>,
    raw_input: Field,
    submit: Html,
}

pub(super) struct Create {
    pages: Pages,
    countries: Vec<Choice>,
    input: AddressInput,
    location: LocationDraft,
    errors: Vec<FieldError>,
    flash: Flash,
    created: bool,
}
impl Create {
    pub async fn new(pages: Pages) -> Result<Self> {
        let location = LocationDraft::new(&pages.addresses, Location::default()).await?;
        Ok(Self {
            countries: pages.countries().await?,
            pages,
            input: AddressInput {
                source: Some("manual".into()),
                verification_status: Some("unverified".into()),
                ..AddressInput::default()
            },
            location,
            errors: Vec::new(),
            flash: Flash::default(),
            created: false,
        })
    }
    async fn take(&mut self, value: &Value) -> Result<()> {
        let fields = form(value, "address");
        let get = |key: &str| fields.get(key).cloned();
        self.location
            .change(&self.pages.addresses, &LocationDraft::posted(&fields))
            .await?;
        self.input = AddressInput {
            label: get("label"),
            phone: get("phone"),
            line1: get("line1"),
            line2: get("line2"),
            line3: get("line3"),
            raw_input: get("raw_input"),
            source: get("source"),
            source_ref: get("source_ref"),
            parser_version: get("parser_version"),
            parse_confidence: get("parse_confidence"),
            verification_status: get("verification_status"),
            ..AddressInput::default()
        };
        self.location.location.apply_to(&mut self.input);
        // The pure shape checks; Geonames membership is checked on save.
        self.errors = model::normalize(&self.input).err().unwrap_or_default();
        Ok(())
    }
    async fn save(&mut self, scope: &Scope) -> Result<()> {
        if !self.errors.is_empty() {
            return Ok(());
        }
        match self.pages.addresses.create(scope, &self.input).await {
            Ok(_) => self.created = true,
            Err(error) if fatal(&error) => return Err(Error::Unauthorized),
            Err(WriteError::Invalid(errors)) => self.errors = errors,
            Err(WriteError::Access(Error::Forbidden)) => self.flash.put(
                StatusKind::Error,
                "You do not have permission to create addresses or write these fields.",
            ),
            Err(_) => self
                .flash
                .put(StatusKind::Error, "The address was not created. Try again."),
        }
        Ok(())
    }
    async fn view(&self, scope: &Scope) -> Result<LiveBody> {
        if self.created {
            return Ok(LiveBody {
                redirect: Some("/addresses".into()),
                ..LiveBody::default()
            });
        }
        let client = self.pages.db.get().await?;
        let masked = bilibi_authz::restrictions_for(&**client, scope, "addresses").await?;
        drop(client);
        let can_create = self.pages.allowed(scope, CREATE).await?;
        let errors = |key: &str| -> Vec<String> {
            self.errors
                .iter()
                .filter(|e| e.field == key)
                .map(|e| e.message.clone())
                .collect()
        };
        let value = |v: &Option<String>| v.clone().unwrap_or_default();
        let field = |key: &str, label: &str, v: &Option<String>| {
            Field::in_form("address", key)
                .id(format!("address-{}", key.replace('_', "-")))
                .label(label)
                .value(value(v))
                .attr("data-debounce", "300")
                .errors(errors(key))
        };
        let bounded = |mut f: Field| {
            f.attrs.maxlength = Some("255".into());
            f
        };
        let i = &self.input;
        let mut contact = vec![bounded(field("label", "Label", &i.label))];
        let mut restricted = Vec::new();
        // A restricted phone renders no input and is never written.
        if masked.contains("phone") {
            restricted.push(RestrictedField::new("address-phone", "Phone"));
        } else {
            contact.push(bounded(
                field("phone", "Phone", &i.phone).kind(InputKind::Tel),
            ));
        }
        let lines = vec![
            field("line1", "Address line 1", &i.line1),
            field("line2", "Address line 2", &i.line2),
            field("line3", "Address line 3", &i.line3),
        ];
        let mut confidence = field("parse_confidence", "Parse confidence", &i.parse_confidence)
            .kind(InputKind::Number);
        confidence.attrs.min = Some("0".into());
        confidence.attrs.max = Some("1".into());
        confidence.attrs.step = Some("0.0001".into());
        let provenance = vec![
            bounded(field("source", "Source", &i.source)),
            bounded(field("source_ref", "Source reference", &i.source_ref)),
            bounded(field("parser_version", "Parser version", &i.parser_version)),
            confidence,
            field(
                "verification_status",
                "Verification status",
                &i.verification_status,
            )
            .kind(InputKind::Select)
            .options(bilibi_ui::choices(&[
                ("Unverified", "unverified"),
                ("Suggested", "suggested"),
                ("Verified", "verified"),
            ]))
            .required(),
        ];
        let header = render(Header {
            title: Html::text("Create Address"),
            subtitle: "Add a tenant-owned address",
            actions: render(BackLink {
                href: "/addresses",
                id: "address-back",
                title: "Back to addresses",
                ..BackLink::default()
            })?,
            ..Header::default()
        })?;
        let html = View {
            flash: self.flash.map(),
            header,
            contact,
            restricted,
            lines,
            location: self.location.render(
                "address",
                &IDS,
                &LABELS,
                &self.countries,
                &self.errors,
            )?,
            provenance,
            raw_input: field("raw_input", "Raw input", &i.raw_input).kind(InputKind::Textarea),
            submit: render(Button {
                id: "address-save",
                button_type: "submit",
                variant: ButtonVariant::Primary,
                disabled: !can_create,
                working: "Creating…",
                body: Html::text("Create Address"),
                ..Button::default()
            })?,
        }
        .render()
        .map_err(|_| Error::Internal)?;
        Ok(LiveBody {
            html,
            ..LiveBody::default()
        })
    }
}
impl LiveState<Scope, Error> for Create {
    fn update<'a>(&'a mut self, request: LiveRequest<'a, Scope>) -> LiveFuture<'a, Error> {
        Box::pin(async move {
            if let LiveInput::Event(value) = request.input {
                match event(value)? {
                    "validate" => self.take(value).await?,
                    "save" if !self.created => {
                        self.flash.clear();
                        self.take(value).await?;
                        self.save(request.scope).await?;
                    }
                    "save" => {}
                    "clear-flash" => self.flash.dismiss(value.get("key").and_then(Value::as_str)),
                    _ => return Err(Error::Invalid),
                }
            }
            self.view(request.scope).await
        })
    }
}
