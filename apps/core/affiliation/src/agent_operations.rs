//! Headless `core.affiliation.*` operations: thin adapters over the owner's
//! transaction API. Reads list and read affiliations under the actor's field
//! restrictions; every write is a reviewed safe write whose preview shares the
//! owner's checks without locking or writing and whose commit runs inside the
//! plan's audited transaction (E2 owns review, replay and the outer commit).
use crate::{
    contributions::*, identity, model::Affiliation, AffiliationId, AffiliationInput, CalendarDate,
    EffectiveDates, OrganisationId, PersonId, Search, Update,
};
use bilibi_agent_api::{
    json::{Data, Output, Page},
    Definitions, Guide, Handler, Operation, OperationClient, OperationFuture, ReadHandler,
    WriteHandler, DEFINITIONS,
};
use bilibi_database::{Db, Error, MutationTransaction};
use bilibi_module_registry::{Contribution, Provenance};
use bilibi_tenancy::{PartyProofs, Scope};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashSet},
    sync::{Arc, OnceLock},
};

struct Bound {
    db: Db,
    proofs: Arc<PartyProofs>,
}
static BOUND: OnceLock<Bound> = OnceLock::new();
pub fn bind(db: Db, proofs: Arc<PartyProofs>) -> bilibi_module_registry::Result<()> {
    BOUND.set(Bound { db, proofs }).map_err(|_| {
        bilibi_module_registry::RegistryError::Service("duplicate affiliation agent binding".into())
    })
}
fn bound() -> bilibi_agent_api::Result<&'static Bound> {
    BOUND.get().ok_or(Error::Internal.into())
}
fn invalid(field: &str, message: &str) -> bilibi_agent_api::Error {
    let mut fields: BTreeMap<String, Vec<String>> = BTreeMap::new();
    fields.insert(field.into(), vec![message.into()]);
    bilibi_agent_api::Error::InvalidInput(fields)
}
fn not_found(error: Error) -> bilibi_agent_api::Error {
    match error {
        Error::NotFound => bilibi_agent_api::Error::NotFound,
        Error::Conflict => invalid("version", "is not the current version"),
        other => other.into(),
    }
}
fn positive(input: &Value, field: &str) -> bilibi_agent_api::Result<i64> {
    input[field]
        .as_i64()
        .filter(|value| *value > 0)
        .ok_or_else(|| invalid(field, "must be a positive integer"))
}
fn id(input: &Value) -> bilibi_agent_api::Result<AffiliationId> {
    AffiliationId::new(positive(input, "affiliation_id")?)
        .map_err(|_| invalid("affiliation_id", "must be a positive integer"))
}
fn date(input: &Value, field: &str) -> bilibi_agent_api::Result<Option<CalendarDate>> {
    match &input[field] {
        Value::Null => Ok(None),
        Value::String(text) if text.trim().is_empty() => Ok(None),
        Value::String(text) => CalendarDate::try_from(text.trim().to_owned())
            .map(Some)
            .map_err(|_| invalid(field, "must be a YYYY-MM-DD calendar date")),
        _ => Err(invalid(field, "must be a YYYY-MM-DD calendar date")),
    }
}
fn dates(input: &Value) -> bilibi_agent_api::Result<EffectiveDates> {
    EffectiveDates::new(date(input, "effective_from")?, date(input, "effective_to")?)
        .map_err(|_| invalid("effective_to", "must be on or after effective_from"))
}
fn text(input: &Value, field: &str) -> bilibi_agent_api::Result<Option<String>> {
    match &input[field] {
        Value::Null => Ok(None),
        Value::String(text) => Ok(Some(text.clone())),
        _ => Err(invalid(field, "must be a string")),
    }
}
fn create_input(input: &Value) -> bilibi_agent_api::Result<AffiliationInput> {
    Ok(AffiliationInput {
        person_id: PersonId::new(positive(input, "person_id")?)
            .map_err(|_| invalid("person_id", "must be a positive integer"))?,
        organisation_id: OrganisationId::new(positive(input, "organisation_id")?)
            .map_err(|_| invalid("organisation_id", "must be a positive integer"))?,
        title: text(input, "title")?,
        work_email: text(input, "work_email")?,
        work_phone: text(input, "work_phone")?,
        dates: dates(input)?,
    })
}
/// The replacement facts of an update keep the record's immutable endpoints.
fn update_input(before: &Affiliation, input: &Value) -> bilibi_agent_api::Result<Update> {
    Ok(Update {
        version: positive(input, "version")?,
        affiliation: AffiliationInput {
            person_id: before.affiliation.person_id,
            organisation_id: before.affiliation.organisation_id,
            title: text(input, "title")?,
            work_email: text(input, "work_email")?,
            work_phone: text(input, "work_phone")?,
            dates: dates(input)?,
        },
    })
}
fn shape(error: Error) -> bilibi_agent_api::Error {
    match error {
        Error::Invalid => invalid(
            "affiliation",
            "title, work contacts or dates are not in the accepted shape",
        ),
        other => not_found(other),
    }
}
fn record(affiliation: Affiliation) -> Data {
    let masked: HashSet<_> = affiliation.restricted.iter().cloned().collect();
    Data::restricted(affiliation.json(), &masked)
}

enum Read {
    List,
    Get,
}
impl ReadHandler for Read {
    fn owner(&self) -> &'static str {
        "core/affiliation"
    }
    fn call<'a>(
        &'a self,
        _client: &'a dyn OperationClient,
        scope: &'a Scope,
        input: &'a Value,
    ) -> OperationFuture<'a> {
        Box::pin(async move {
            let client = bound()?.db.get().await.map_err(Error::from)?;
            match self {
                Self::List => {
                    let query: Search = serde_json::from_value(input.clone())
                        .map_err(|_| invalid("query", "has an unknown or malformed field"))?;
                    let page = identity::search(&**client, scope, &query)
                        .await
                        .map_err(shape)?;
                    Ok(Output::Page(Page {
                        entries: Data::Array(page.entries.into_iter().map(record).collect()),
                        page: page.page as u64,
                        page_size: page.page_size as u64,
                        total: page.total as u64,
                    }))
                }
                Self::Get => Ok(Output::Data(record(
                    identity::read(&**client, scope, id(input)?)
                        .await
                        .map_err(not_found)?,
                ))),
            }
        })
    }
}

enum Write {
    Create,
    Update,
    End,
    Unlink,
}
impl WriteHandler for Write {
    fn owner(&self) -> &'static str {
        "core/affiliation"
    }
    fn preview<'a>(
        &'a self,
        _client: &'a dyn OperationClient,
        scope: &'a Scope,
        input: &'a Value,
    ) -> OperationFuture<'a> {
        Box::pin(async move {
            let bound = bound()?;
            let client = bound.db.get().await.map_err(Error::from)?;
            let client = &**client;
            let value = match self {
                Self::Create => {
                    let input = identity::preview_create(
                        client,
                        scope,
                        &bound.proofs,
                        &create_input(input)?,
                    )
                    .await
                    .map_err(shape)?;
                    json!({"summary":format!("Affiliate person {} with organisation {}; no account, employment or grant follows", input.person_id, input.organisation_id),"affiliation":input})
                }
                Self::Update => {
                    let before = identity::preview_write(
                        client,
                        scope,
                        id(input)?,
                        positive(input, "version")?,
                        UPDATE,
                    )
                    .await
                    .map_err(not_found)?;
                    let update = update_input(&before, input)?;
                    json!({"summary":"Replace an affiliation's title, visible work contacts and dates","before":record(before).encode(),"affiliation":update.affiliation,"version":update.version})
                }
                Self::End => {
                    let before = identity::preview_write(
                        client,
                        scope,
                        id(input)?,
                        positive(input, "version")?,
                        UPDATE,
                    )
                    .await
                    .map_err(not_found)?;
                    let to = date(input, "effective_to")?
                        .ok_or_else(|| invalid("effective_to", "is required"))?;
                    json!({"summary":format!("End an affiliation on {}; identity, contacts and history are kept", to.as_str()),"before":record(before).encode(),"effective_to":to})
                }
                Self::Unlink => {
                    let before = identity::preview_write(
                        client,
                        scope,
                        id(input)?,
                        positive(input, "version")?,
                        UNLINK,
                    )
                    .await
                    .map_err(not_found)?;
                    json!({"summary":"Unlink an affiliation; the dated record is retained and refuses further edits","before":record(before).encode()})
                }
            };
            Ok(value.into())
        })
    }
    fn commit<'a>(
        &'a self,
        tx: &'a mut MutationTransaction<'_>,
        scope: &'a Scope,
        input: &'a Value,
    ) -> OperationFuture<'a> {
        Box::pin(async move {
            let affiliation = match self {
                Self::Create => identity::create(tx, scope, &create_input(input)?).await,
                Self::Update => {
                    let id = id(input)?;
                    let version = positive(input, "version")?;
                    let before = identity::preview_write(tx.sql(), scope, id, version, UPDATE)
                        .await
                        .map_err(not_found)?;
                    identity::update(tx, scope, id, &update_input(&before, input)?).await
                }
                Self::End => {
                    let to = date(input, "effective_to")?
                        .ok_or_else(|| invalid("effective_to", "is required"))?;
                    identity::end(tx, scope, id(input)?, positive(input, "version")?, to).await
                }
                Self::Unlink => {
                    identity::unlink(tx, scope, id(input)?, positive(input, "version")?).await
                }
            }
            .map_err(shape)?;
            Ok(Output::Data(record(affiliation)))
        })
    }
}

fn id_schema() -> Value {
    json!({"type":"integer","minimum":1})
}
fn date_schema() -> Value {
    json!({"type":"string","format":"date"})
}
fn facts() -> serde_json::Map<String, Value> {
    json!({
        "title":{"type":"string","maxLength":160},
        "work_email":{"type":"string","maxLength":254},
        "work_phone":{"type":"string","maxLength":64},
        "effective_from":date_schema(),
        "effective_to":date_schema()
    })
    .as_object()
    .cloned()
    .expect("object")
}
fn list_schema() -> Value {
    json!({"type":"object","properties":{
        "person_id":id_schema(),"organisation_id":id_schema(),
        "include_unlinked":{"type":"boolean"},"on":date_schema(),
        "page":{"type":"integer","minimum":1,"maximum":1000000},"page_size":{"type":"integer","minimum":1,"maximum":100}
    }})
}
fn get_schema() -> Value {
    json!({"type":"object","required":["affiliation_id"],"properties":{"affiliation_id":id_schema()}})
}
fn create_schema() -> Value {
    let mut properties = facts();
    properties.insert("person_id".into(), id_schema());
    properties.insert("organisation_id".into(), id_schema());
    json!({"type":"object","required":["person_id","organisation_id"],"properties":properties})
}
fn update_schema() -> Value {
    let mut properties = facts();
    properties.insert("affiliation_id".into(), id_schema());
    properties.insert("version".into(), id_schema());
    json!({"type":"object","required":["affiliation_id","version"],"properties":properties})
}
fn end_schema() -> Value {
    json!({"type":"object","required":["affiliation_id","version","effective_to"],"properties":{"affiliation_id":id_schema(),"version":id_schema(),"effective_to":date_schema()}})
}
fn unlink_schema() -> Value {
    json!({"type":"object","required":["affiliation_id","version"],"properties":{"affiliation_id":id_schema(),"version":id_schema()}})
}
fn one_output() -> Value {
    json!({"type":"object","description":"One affiliation: person_id, organisation_id, title, work contacts, inclusive effective_from/effective_to, unlinked_at and version. A hidden work contact reads as {\"restricted\":true}."})
}
fn many_output() -> Value {
    json!({"type":"array","description":"Bounded affiliations with work contacts redacted under your current role restrictions."})
}
inventory::submit! { Contribution::new(DEFINITIONS, "core/affiliation.agent", Provenance { module: "core/affiliation", source: file!(), line: line!() }, &Definitions {
    operations: &[
        Operation { key:"core.affiliation.list", title:"List affiliations", summary:"List a person's or an organisation's dated work affiliations, optionally on one calendar day.", keywords:&["affiliation","contact","organisation","person","work"], capability:LIST, input:list_schema, output:many_output, handler:Handler::Read(&Read::List) },
        Operation { key:"core.affiliation.get", title:"Read an affiliation", summary:"Read one affiliation with its visible work contacts and dates.", keywords:&["affiliation","work","contact"], capability:VIEW, input:get_schema, output:one_output, handler:Handler::Read(&Read::Get) },
        Operation { key:"core.affiliation.create", title:"Affiliate a person with an organisation", summary:"Record a dated position with optional title and work contacts. Creates no account, employment or grant.", keywords:&["affiliation","add","position","work","contact"], capability:CREATE, input:create_schema, output:one_output, handler:Handler::SafeWrite(&Write::Create) },
        Operation { key:"core.affiliation.update", title:"Update an affiliation", summary:"Replace the title, visible work contacts and dates using the current version; person and organisation never change.", keywords:&["affiliation","edit","title","dates"], capability:UPDATE, input:update_schema, output:one_output, handler:Handler::SafeWrite(&Write::Update) },
        Operation { key:"core.affiliation.end", title:"End an affiliation", summary:"Set the inclusive end date; identity, contacts and history are kept.", keywords:&["affiliation","end","leave"], capability:UPDATE, input:end_schema, output:one_output, handler:Handler::SafeWrite(&Write::End) },
        Operation { key:"core.affiliation.unlink", title:"Unlink an affiliation", summary:"Retain the dated record while refusing further edits and binding proofs. Never deletes.", keywords:&["affiliation","unlink","remove"], capability:UNLINK, input:unlink_schema, output:one_output, handler:Handler::SafeWrite(&Write::Unlink) },
    ],
    guides: &[Guide { key:"core.affiliation", title:"Work affiliations", summary:"Dated person-to-organisation positions with work contacts, independent of accounts and People employment.", keywords:&["affiliation","work","contact","organisation","person","employment"], capability:LIST,
        body:"An affiliation links one Person to one Organisation for a dated position: optional title, work email and work phone, and inclusive effective_from/effective_to calendar dates (YYYY-MM-DD); a missing bound is open. A person may hold several affiliations at once, including several positions at one organisation. Work contacts never replace personal contacts (core.person.*), and no affiliation creates a login, grant or employment: People owns Employment and points at an affiliation with our own company.\n\nList with core.affiliation.list by person_id or organisation_id; add on=YYYY-MM-DD to keep only affiliations in effect that day, and include_unlinked=true to see retained unlinked records. Create, update, end and unlink go through safe-write review and plan-code commit. Update sends affiliation_id, the current version and the replacement title, work_email, work_phone, effective_from and effective_to; an omitted or empty visible contact clears it, and a hidden contact (read as restricted) must be omitted and is preserved. The person and organisation of an affiliation never change: create another affiliation to move someone. End sets effective_to; unlink keeps the record read-only. Archived companies and retired or protected system persons refuse new or changed affiliations." }],
}) }
