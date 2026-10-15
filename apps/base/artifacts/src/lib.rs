//! Private business documents. Call outside an owner's transaction: durable
//! reservations/tombstones must commit independently of filesystem work.
mod contributions;
pub mod pdf;
mod storage;

use bilibi_database::{Change, MutationBoundary, MutationTransaction};
use bilibi_module_registry::Service;
use bilibi_party_ids::OrganisationId;
use bilibi_settings::Settings;
use bilibi_tenancy::{Authentication, ProofClient, ProofFuture, Scope};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{any::type_name, future::Future, pin::Pin, sync::Arc};
use tokio_postgres::{Client, GenericClient, Row};

pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Database(#[from] bilibi_database::Error),
    #[error("Database unavailable")]
    Sql(#[from] tokio_postgres::Error),
    #[error("Storage unavailable; reservation or tombstone retained for recovery")]
    Storage(#[from] std::io::Error),
    #[error("Storage root must be an existing private 0700 absolute directory")]
    UnsafeRoot,
    #[error("Configure storage and a positive retention period first")]
    Policy,
    #[error("Invalid artifact input")]
    Invalid,
    #[error("Invalid PDF document structure")]
    InvalidDocument,
    #[error("Text contains characters the built-in PDF font cannot show")]
    UnsupportedText,
    #[error("Not a structurally valid PDF")]
    InvalidPdf,
    #[error("Artifact not found")]
    NotFound,
    #[error("Artifact integrity failure")]
    Integrity,
    #[error("Physical cleanup pending")]
    CleanupPending,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    Create,
    Read,
    Delete,
    Purge,
    Retain,
}
#[derive(Clone, Debug)]
pub struct Reference {
    pub subject: String,
    pub kind: String,
}
/// Select a concrete adapter in trusted server code, never from request input.
/// Check live company membership/state and current subject permissions on every
/// callback. Purge has no reference and requires company maintenance authority.
/// Base binds both this stable owner ID and the concrete Rust adapter type.
pub trait Owner: Send + Sync + 'static {
    fn owner_id(&self) -> &'static str;
    fn authorize<'a>(
        &'a self,
        client: &'a dyn ProofClient,
        scope: &'a Scope,
        company: OrganisationId,
        operation: Operation,
        reference: Option<&'a Reference>,
    ) -> ProofFuture<'a, ()>;
}
pub type RenderFuture<'a> = Pin<Box<dyn Future<Output = Result<Vec<u8>>> + Send + 'a>>;
/// Implement on an owner that generates PDFs. Base authorizes `Create` before
/// calling `render_pdf`, which selects only the data the scope's actor may see
/// from the caller's `data` and its own records, and returns complete PDF bytes
/// (use `pdf::render` for text and tables). Never log document contents.
pub trait PdfOwner: Owner {
    fn render_pdf<'a>(
        &'a self,
        scope: &'a Scope,
        company: OrganisationId,
        reference: &'a Reference,
        data: &'a Value,
    ) -> RenderFuture<'a>;
}
/// No storage location or raw database row escapes the facade.
#[derive(Clone, Debug)]
pub struct Metadata {
    pub id: String,
    pub reference: Reference,
    pub content_type: String,
    pub byte_size: i64,
    pub sha256: String,
    pub expires_at: String,
}
/// Intentionally has no Debug implementation: document contents are private.
pub struct Document {
    pub metadata: Metadata,
    pub bytes: Vec<u8>,
}
struct Stored {
    key: i64,
    metadata: Metadata,
    root: String,
    ready: bool,
    deleted: bool,
    purged: bool,
    expired: bool,
}
const COLUMNS: &str = "id,public_id,subject,kind,content_type,byte_size,sha256,storage_root,expires_at::text,ready_at IS NOT NULL AS ready,deleted_at IS NOT NULL AS deleted,purged_at IS NOT NULL AS purged,expires_at <= clock_timestamp() AS expired";
fn stored(row: Row) -> Stored {
    Stored {
        key: row.get("id"),
        metadata: Metadata {
            id: row.get("public_id"),
            reference: Reference {
                subject: row.get("subject"),
                kind: row.get("kind"),
            },
            content_type: row.get("content_type"),
            byte_size: row.get("byte_size"),
            sha256: row.get("sha256"),
            expires_at: row.get("expires_at"),
        },
        root: row.get("storage_root"),
        ready: row.get("ready"),
        deleted: row.get("deleted"),
        purged: row.get("purged"),
        expired: row.get("expired"),
    }
}
fn evidence(row: &Stored) -> Value {
    // Deliberately exclude subject, kind, content, MIME, digest and path.
    json!({"artifact_id":row.metadata.id,"ready":row.ready,"deleted":row.deleted,"purged":row.purged})
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn bounded(text: &str, max: usize) -> bool {
    !text.is_empty() && text.len() <= max && !text.chars().any(char::is_control)
}

pub struct Artifacts {
    boundary: Arc<MutationBoundary>,
    settings: Arc<Settings>,
    authentication: Arc<Authentication>,
}
impl Artifacts {
    pub fn for_owner<'a, O: Owner>(
        &'a self,
        scope: &'a Scope,
        company: OrganisationId,
        owner: &'a O,
    ) -> Owned<'a, O> {
        Owned {
            store: self,
            scope,
            company,
            owner,
        }
    }
}
/// Scoped facade; the concrete adapter cannot be swapped by a client string.
pub struct Owned<'a, O: Owner> {
    store: &'a Artifacts,
    scope: &'a Scope,
    company: OrganisationId,
    owner: &'a O,
}
const PDF: &str = "application/pdf";
impl<O: PdfOwner> Owned<'_, O> {
    /// Authorize, render through the owner, validate the output and store it as
    /// a private `application/pdf` artifact. Same call rules as `put`.
    pub async fn generate_pdf(
        &self,
        client: &mut Client,
        reference: Reference,
        data: &Value,
    ) -> Result<Metadata> {
        if !data.is_object() {
            return Err(Error::Invalid);
        }
        self.authorize(&*client, Operation::Create, Some(&reference))
            .await?;
        let bytes = self
            .owner
            .render_pdf(self.scope, self.company, &reference, data)
            .await?;
        self.store_pdf(client, reference, &bytes).await
    }
    /// Store PDF bytes made elsewhere (for example by a later HTML engine)
    /// after the same structural validation; `put` rechecks authority.
    pub async fn store_pdf(
        &self,
        client: &mut Client,
        reference: Reference,
        bytes: &[u8],
    ) -> Result<Metadata> {
        pdf::validate(bytes)?;
        self.put(client, reference, bytes, PDF).await
    }
}
#[derive(Debug, Default)]
pub struct Recovery {
    pub purged: Vec<String>,
    pub failed: Vec<(String, Error)>,
}
impl<O: Owner> Owned<'_, O> {
    async fn authorize(
        &self,
        client: &(impl GenericClient + Sync),
        operation: Operation,
        reference: Option<&Reference>,
    ) -> Result<()> {
        if !bounded(self.owner.owner_id(), 255) || type_name::<O>().len() > 1024 {
            return Err(Error::Invalid);
        }
        let live = self
            .store
            .authentication
            .authenticate(
                client,
                self.scope.tenant(),
                self.scope.user(),
                String::new(),
            )
            .await?;
        if live.actor().person() != self.scope.actor().person()
            || live.actor().organisation() != self.scope.actor().organisation()
            || live.actor().affiliation() != self.scope.actor().affiliation()
            || live
                .actor()
                .organisation()
                .is_some_and(|id| id != self.company)
        {
            return Err(bilibi_database::Error::Forbidden.into());
        }
        self.owner
            .authorize(client, &live, self.company, operation, reference)
            .await?;
        Ok(())
    }
    async fn fetch(&self, tx: &MutationTransaction<'_>, id: &str) -> Result<Stored> {
        tx.sql().query_opt(&format!("SELECT {COLUMNS} FROM base_artifacts WHERE public_id=$1 AND tenant_id=$2 AND company_id=$3 AND owner_id=$4 AND owner_adapter=$5 FOR UPDATE"), &[&id,&self.scope.tenant(),&self.company.get(),&self.owner.owner_id(),&type_name::<O>()]).await?.map(stored).ok_or(Error::NotFound)
    }
    async fn begin<'c>(
        &self,
        client: &'c mut Client,
        action: &str,
    ) -> Result<MutationTransaction<'c>> {
        Ok(self
            .store
            .boundary
            .begin(client, self.scope.mutation_context(action))
            .await?)
    }
    async fn audit(&self, tx: &MutationTransaction<'_>, row: &Stored, action: &str) -> Result<()> {
        bilibi_audit::record_action(tx.sql(), &bilibi_audit::Attribution::from_context(tx.context())?, None, action,
            json!({"artifact_id":row.metadata.id,"company_id":self.company.get(),"owner_id":self.owner.owner_id()}), false).await?;
        Ok(())
    }
    async fn transition(
        &self,
        tx: &mut MutationTransaction<'_>,
        row: &Stored,
        column: &str,
    ) -> Result<Stored> {
        // column is chosen only by this module; never accept an SQL identifier.
        let query = match column {
            "ready" => "UPDATE base_artifacts SET ready_at=clock_timestamp() WHERE id=$1",
            "deleted" => "UPDATE base_artifacts SET deleted_at=clock_timestamp() WHERE id=$1",
            "purged" => "UPDATE base_artifacts SET purged_at=clock_timestamp() WHERE id=$1",
            _ => return Err(Error::Invalid),
        };
        tx.sql().execute(query, &[&row.key]).await?;
        let next = self.fetch(tx, &row.metadata.id).await?;
        tx.record(Change {
            kind: "base/artifacts.document".into(),
            id: row.key,
            old: evidence(row),
            new: evidence(&next),
        })
        .await?;
        Ok(next)
    }
    /// Reserve metadata durably, write private bytes exclusively, then publish
    /// under the row lock. A failure/ambiguous commit leaves tracked metadata;
    /// recovery never removes bytes belonging to a ready document.
    pub async fn put(
        &self,
        client: &mut Client,
        reference: Reference,
        bytes: &[u8],
        content_type: &str,
    ) -> Result<Metadata> {
        if !bounded(&reference.subject, 255)
            || !bounded(&reference.kind, 100)
            || !bounded(content_type, 255)
            || bytes.is_empty()
            || bytes.len() > storage::MAX_BYTES
        {
            return Err(Error::Invalid);
        }
        let mut tx = self.begin(client, "artifacts.reserve").await?;
        self.authorize(tx.sql(), Operation::Create, Some(&reference))
            .await?;
        let root = self
            .store
            .settings
            .get_global(tx.sql(), "artifacts.storage_root")
            .await?
            .as_str()
            .ok_or(Error::Policy)?
            .to_owned();
        let days = self
            .store
            .settings
            .get_global(tx.sql(), "artifacts.retention_days")
            .await?
            .as_i64()
            .filter(|d| *d > 0 && *d <= 36500)
            .ok_or(Error::Policy)?;
        let max = self
            .store
            .settings
            .get_global(tx.sql(), "artifacts.max_bytes")
            .await?
            .as_u64()
            .ok_or(Error::Policy)?;
        if bytes.len() as u64 > max {
            return Err(Error::Invalid);
        }
        storage::io(|| storage::validate_root(&root))?;
        let id = uuid::Uuid::new_v4().to_string();
        let size = bytes.len() as i64;
        let sha256 = digest(bytes);
        let row = tx.sql().query_one(&format!("INSERT INTO base_artifacts(public_id,tenant_id,company_id,owner_id,owner_adapter,subject,kind,content_type,byte_size,sha256,storage_root,expires_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,clock_timestamp()+$12::bigint * interval '1 day') RETURNING {COLUMNS}"), &[&id,&self.scope.tenant(),&self.company.get(),&self.owner.owner_id(),&type_name::<O>(),&reference.subject,&reference.kind,&content_type,&size,&sha256,&root,&days]).await?;
        let row = stored(row);
        tx.record(Change {
            kind: "base/artifacts.document".into(),
            id: row.key,
            old: Value::Null,
            new: evidence(&row),
        })
        .await?;
        self.audit(&tx, &row, "artifacts.reserve").await?;
        tx.commit().await?;
        let mut tx = self.begin(client, "artifacts.publish").await?;
        let row = self.fetch(&tx, &id).await?;
        self.authorize(tx.sql(), Operation::Create, Some(&row.metadata.reference))
            .await?;
        if row.deleted || row.ready || row.expired {
            return Err(Error::NotFound);
        }
        // Keep this IO synchronous: cancellation must not leave an in-flight
        // writer running after the DB lock is dropped and recovery purges it.
        storage::io(|| storage::write(&row.root, &id, bytes))?;
        self.authorize(tx.sql(), Operation::Create, Some(&row.metadata.reference))
            .await?;
        let row = self.transition(&mut tx, &row, "ready").await?;
        self.audit(&tx, &row, "artifacts.publish").await?;
        tx.commit().await?;
        Ok(row.metadata)
    }
    /// Lock, reauthorize, verify byte count/digest, and commit read evidence
    /// before returning any bytes. Never cache this result for another caller.
    pub async fn read(&self, client: &mut Client, id: &str) -> Result<Document> {
        let tx = self.begin(client, "artifacts.read").await?;
        let row = self.fetch(&tx, id).await?;
        self.authorize(tx.sql(), Operation::Read, Some(&row.metadata.reference))
            .await?;
        if !row.ready || row.deleted || row.expired {
            return Err(Error::NotFound);
        }
        let bytes = storage::io(|| storage::read(&row.root, id, row.metadata.byte_size as usize))?;
        if digest(&bytes) != row.metadata.sha256 {
            return Err(Error::Integrity);
        }
        self.audit(&tx, &row, "artifacts.read").await?;
        tx.commit().await?;
        Ok(Document {
            metadata: row.metadata,
            bytes,
        })
    }
    /// A tombstone commits before unlink. Failed unlink/commit is retried
    /// idempotently; a missing file still requires successful audit to purge.
    pub async fn delete(&self, client: &mut Client, id: &str) -> Result<()> {
        let mut tx = self.begin(client, "artifacts.delete").await?;
        let row = self.fetch(&tx, id).await?;
        self.authorize(tx.sql(), Operation::Delete, Some(&row.metadata.reference))
            .await?;
        if !row.deleted {
            let row = self.transition(&mut tx, &row, "deleted").await?;
            self.audit(&tx, &row, "artifacts.delete").await?;
        }
        tx.commit().await?;
        self.cleanup(client, id).await
    }
    async fn cleanup(&self, client: &mut Client, id: &str) -> Result<()> {
        let mut tx = self.begin(client, "artifacts.purge").await?;
        let row = self.fetch(&tx, id).await?;
        self.authorize(tx.sql(), Operation::Delete, Some(&row.metadata.reference))
            .await?;
        if !row.deleted {
            return Err(Error::Invalid);
        }
        if !row.purged {
            storage::io(|| storage::delete(&row.root, id))?;
            let row = self.transition(&mut tx, &row, "purged").await?;
            self.audit(&tx, &row, "artifacts.purge").await?;
        }
        tx.commit().await?;
        Ok(())
    }
    /// Owner-scoped bounded maintenance. Reconciles tombstones and reservations
    /// older than one hour; ready documents are left to A2 retention policy.
    /// Candidate locking rechecks state so a delayed writer cannot be purged.
    pub async fn recover(&self, client: &mut Client, limit: i64) -> Result<Recovery> {
        if !(1..=1000).contains(&limit) {
            return Err(Error::Invalid);
        }
        self.authorize(&*client, Operation::Purge, None).await?;
        let rows = client.query("SELECT public_id FROM base_artifacts WHERE tenant_id=$1 AND company_id=$2 AND owner_id=$3 AND owner_adapter=$4 AND purged_at IS NULL AND (deleted_at IS NOT NULL OR (ready_at IS NULL AND created_at < clock_timestamp()-interval '1 hour')) ORDER BY created_at,id LIMIT $5", &[&self.scope.tenant(),&self.company.get(),&self.owner.owner_id(),&type_name::<O>(),&limit]).await?;
        let mut report = Recovery::default();
        for candidate in rows {
            let id: String = candidate.get(0);
            match self.recover_one(client, &id).await {
                Ok(true) => report.purged.push(id),
                Ok(false) => {}
                Err(error) => report.failed.push((id, error)),
            }
        }
        Ok(report)
    }
    async fn recover_one(&self, client: &mut Client, id: &str) -> Result<bool> {
        let mut tx = self.begin(client, "artifacts.recover").await?;
        let row = self.fetch(&tx, id).await?;
        self.authorize(tx.sql(), Operation::Purge, None).await?;
        self.authorize(tx.sql(), Operation::Delete, Some(&row.metadata.reference))
            .await?;
        if row.purged || (row.ready && !row.deleted) {
            return Ok(false);
        }
        if !row.deleted {
            let row = self.transition(&mut tx, &row, "deleted").await?;
            self.audit(&tx, &row, "artifacts.delete").await?;
        }
        tx.commit().await?;
        self.cleanup(client, id).await?;
        Ok(true)
    }
}
inventory::submit! {
    bilibi_module_registry::Module {
        id:"base/artifacts",layer:bilibi_module_registry::Layer::Base,
        dependencies:&["base/audit","base/authz","base/database","base/settings","base/tenancy","base/party_ids"],
        migrations:&[("base-artifacts-001",include_str!("../migrations/001-artifacts.sql"))],
        services:&[Service {id:"base/artifacts.store",factory:|services| Ok(Arc::new(Artifacts {
            boundary:services.get("base/database.mutations")?, settings:services.get("base/settings.store")?, authentication:services.get("base/tenancy.authentication")?,
        }))}],
    }
}
