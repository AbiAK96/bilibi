//! Reference provider of an optional port, the shape Purchasing's
//! `supply_chain/purchasing.order_line` takes for Payables' three-way match.
//! It owns one table and one capability, and answers
//! `port_reference/orders.order_line` through its own facade on the caller's
//! client and scope. Mount it only to prove `base/ports`
//! (`apps/web/examples/ports-reference.rs`).
use bilibi_authz::{
    authorize_key, CapabilityDefinition, RoleDefinition, VocabularyContribution, VOCABULARY,
};
use bilibi_database::{Error, Result};
use bilibi_module_registry::{Contribution, Layer, Module, Provenance};
use bilibi_ports::{
    answer, on_client, request, Port, PortClient, PortFuture, PortProvider, Value, PORTS,
};
use bilibi_tenancy::Scope;
use serde::{Deserialize, Serialize};
use tokio_postgres::GenericClient;

const MODULE: &str = "port_reference/orders";
pub const VIEW: &str = "port-reference.orders.view";

macro_rules! provenance {
    () => {
        Provenance {
            module: MODULE,
            source: file!(),
            line: line!(),
        }
    };
}

inventory::submit! {
    Module {
        id: MODULE, layer: Layer::Domains,
        dependencies: &["base/authz", "base/database", "base/ports", "base/tenancy"],
        migrations: &[("port-reference-orders-001-order-lines", include_str!("../migrations/001-order-lines.sql"))],
        services: &[],
    }
}
inventory::submit! { Contribution::new(VOCABULARY, "port_reference/orders.vocabulary", provenance!(), &VocabularyContribution {
    domains: &["port-reference"],
    verbs: &[],
    capabilities: &[CapabilityDefinition::tenant(VIEW)],
    roles: &[RoleDefinition { code: "tenant_owner", name: "Tenant Owner", description: "Contributed system role; reconciled explicitly", grant_all: false, capabilities: &[VIEW] }],
    system_principals: &[],
}) }

/// The documented answer: one order line as the provider sees it.
#[derive(Serialize)]
pub struct OrderLine {
    pub id: i64,
    pub order_number: String,
    pub quantity: String,
    pub unit_price: String,
    pub currency_code: String,
    pub status: String,
}

/// The facade read: the provider's capability, then a line of a company in the
/// scope's tenant. A missing or other-tenant line is `NotFound`.
pub async fn order_line(
    client: &impl GenericClient,
    scope: &Scope,
    company_id: i64,
    line_id: i64,
) -> Result<OrderLine> {
    authorize_key(client, scope, VIEW).await?;
    let row = client
        .query_opt(
            "SELECT id,order_number,quantity::text,unit_price::text,currency_code,status FROM port_reference_order_lines WHERE tenant_id=$1 AND company_id=$2 AND id=$3",
            &[&scope.tenant(), &company_id, &line_id],
        )
        .await?
        .ok_or(Error::NotFound)?;
    Ok(OrderLine {
        id: row.get(0),
        order_number: row.get(1),
        quantity: row.get(2),
        unit_price: row.get(3),
        currency_code: row.get(4),
        status: row.get(5),
    })
}

/// The documented request.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OrderLineRequest {
    company_id: i64,
    line_id: i64,
}

struct OrderLinePort;
impl PortProvider for OrderLinePort {
    fn call<'a>(&'a self, client: PortClient<'a>, scope: &'a Scope, body: Value) -> PortFuture<'a> {
        Box::pin(async move {
            let OrderLineRequest {
                company_id,
                line_id,
            } = request(body)?;
            let line = on_client!(client, |c| order_line(c, scope, company_id, line_id).await)?;
            answer(&line)
        })
    }
}
inventory::submit! { Contribution::new(PORTS, "port_reference/orders.order_line", provenance!(), &Port { provider: &OrderLinePort }) }
