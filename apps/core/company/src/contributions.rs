//! Reuse the registry's route contract; Web owns dispatch and scope guards.
use bilibi_menu::{MenuItem, Requirement};
use bilibi_module_registry::{
    contributions::{Access, Method, Route, ROUTES},
    Contribution, Provenance,
};

inventory::submit! {
    Contribution::new(ROUTES, "core/company.list", Provenance { module: "core/company", source: file!(), line: line!() }, &Route {
        method: Method::Get, path: "/companies", service: "core/company.pages", operation: "list", access: Access::Capability("companies.view"), framed: true,
    })
}
inventory::submit! {
    Contribution::new(ROUTES, "core/company.detail", Provenance { module: "core/company", source: file!(), line: line!() }, &Route {
        method: Method::Get, path: "/companies/{id}", service: "core/company.pages", operation: "detail", access: Access::Capability("companies.view"), framed: true,
    })
}
bilibi_menu::contribute!(
    "core/company",
    MenuItem::page("admin.company", "Companies", "/companies")
        .icon("building-office-2")
        .under("admin")
        .requires(Requirement::Capability("companies.view"))
        .order(10)
);
