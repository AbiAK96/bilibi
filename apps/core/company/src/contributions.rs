//! Reuse the registry's route contract; Web owns dispatch and scope guards.
use bilibi_module_registry::{
    contributions::{Access, Method, Route, ROUTES},
    Contribution, Provenance,
};

inventory::submit! {
    Contribution::new(ROUTES, "core/company.list", Provenance { module: "core/company", source: file!(), line: line!() }, &Route {
        method: Method::Get, path: "/companies", service: "core/company.companies", operation: "list", access: Access::Capability("companies.view"), framed: true,
    })
}
inventory::submit! {
    Contribution::new(ROUTES, "core/company.detail", Provenance { module: "core/company", source: file!(), line: line!() }, &Route {
        method: Method::Get, path: "/companies/{id}", service: "core/company.companies", operation: "detail", access: Access::Capability("companies.view"), framed: true,
    })
}
