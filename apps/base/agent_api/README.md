# Agent operation boundary

Use the `base/agent_api.registry` service for search, describe, guide and read
dispatch. Definitions are discovered through the typed `Definitions` consumer
in [src/lib.rs](src/lib.rs); module owners contribute their operations and guides
with the registry's `Contribution` and source `Provenance`. No central host list
or module-specific endpoint is needed. [src/reference.rs](src/reference.rs) is a
complete non-business contribution example.

Keys start with the contributing module ID with `/` changed to `.`. Operations
need an additional segment; guides may use the module prefix itself. Keys must
be unique across operations and guides. Discovery rejects malformed metadata,
foreign handler ownership, unsupported schema keywords, undeclared capabilities,
read operations on write capabilities, and authority administration capabilities.
Titles have a 120-character limit, summaries 500, guide bodies 8 KiB. Registry
entries retain source provenance and materialized schemas.

The host passes a fresh sealed `tenancy::Scope` authenticated through the live
account/session or credential owner. E1 deliberately uses the existing proven
account actor; K9 supplies Person/Organisation facets and E5 supplies production
OAuth/HTTP adapters. System scopes cannot enter this API. Never deserialize a
scope, infer an actor from operation input, or cache reach across requests.

Search uses one fresh capability snapshot without per-candidate decision logs.
Direct deny wins over grants and `grant_all`; platform capabilities additionally
need explicit operator identity. Role/grant/field administration, impersonation,
settings and agent-connection management cannot be agent reach. Search defaults
to ten results, clamps limits to 1–50, supports a read/write filter and optional
unavailable entries. Rankings follow Bilimbi's token stemming, field weights and
key tie breaker. Describe and guide reads also check fresh reach.

Read dispatch validates the declared input, caps `page_size` at 100, asks normal
logged Authz authorization, then calls the owner handler. Owners continue to
call their scoped facade and enforce field restrictions. All direct writes are
refused. Unsupported writes remain discoverable; the `WriteHandler` contract
provides preview and a commit on the shared audited `MutationTransaction` for
E2's future plan service. E1 exposes no write execution or approval bypass.

The [schema dialect](src/schema.rs) accepts object/properties/required,
string/enum/maxLength/format, integer/number/enum/minimum/maximum, boolean and
array/items, plus title/description annotations. Every node needs a type and
root input must be an object. Input objects refuse unknown fields. Validation
returns dotted field paths, including array indexes. Null is not implicit.
String formats are date, date-time with offset, and email. Nesting is bounded
at discovery. Output schemas describe data without runtime enforcement, as in
Bilimbi.

Use [json::Data](src/json.rs) for results and its `Restricted` variant for a
withheld field. The marker stores no original value and serializes only as
`{"restricted":true}`. `Data::restricted` replaces restricted field values
before they enter an output. Owners supply typed dates/times as ISO strings and
exact decimals as strings; Rust's JSON value type excludes functions and opaque
runtime handles. Pages carry page/page_size/total metadata. Every envelope has
the untrusted-content notice.

For a runtime walkthrough, migrate and seed an isolated database, then run:

```sh
BILIBI_AGENT_DEMO=disposable cargo run -p bilibi-agent-api --example agent-walkthrough --locked
```

Set `DATABASE_URL` to that disposable database. This runs real `bilibi_web::App`
services and authenticates demo credentials. It prints search, schemas, guide,
restricted results, malformed input, unavailable entries, direct-write refusal,
live grant/revocation, cross-tenant denial, deny precedence and decision logs.
Temporary grants and sessions are removed on successful completion. This is a
manual runtime specimen, with no unit tests or assertions.
