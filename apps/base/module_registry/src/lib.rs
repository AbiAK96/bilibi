//! Deep modules submit descriptors at link time. Discovery validates the graph,
//! sorts it dependency-first, and builds named typed services without host lists.
use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
pub mod contributions;
pub use contributions::{Catalog, Consumer, Contribution, Entry, Provenance};

pub type Value = Arc<dyn Any + Send + Sync>;
pub type Result<T> = std::result::Result<T, RegistryError>;
pub type Factory = fn(&Services) -> Result<Value>;
pub type Migration = (&'static str, &'static str);

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("Invalid module graph: {0}")]
    Graph(String),
    #[error("Invalid contribution: {0}")]
    Contribution(String),
    #[error("Missing or wrong service type: {0}")]
    Service(String),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Layer {
    Base,
    Core,
    Domains,
    Extensions,
}
impl Layer {
    fn prefix(self) -> &'static str {
        match self {
            Self::Base => "base/",
            Self::Core => "core/",
            Self::Domains => "domains/",
            Self::Extensions => "extensions/",
        }
    }
}
pub struct Service {
    pub id: &'static str,
    pub factory: Factory,
}
pub struct Module {
    pub id: &'static str,
    pub layer: Layer,
    pub dependencies: &'static [&'static str],
    pub migrations: &'static [Migration],
    pub services: &'static [Service],
}
inventory::collect!(Module);

pub fn discover() -> Result<Vec<&'static Module>> {
    let modules = order(inventory::iter::<Module>.into_iter().collect())?;
    Catalog::discover(&modules)?;
    Ok(modules)
}

pub fn order(modules: Vec<&'static Module>) -> Result<Vec<&'static Module>> {
    let mut pending = BTreeMap::new();
    let mut services = BTreeSet::new();
    let mut migrations = BTreeSet::new();
    for module in modules {
        if !module.id.starts_with(module.layer.prefix())
            || pending.insert(module.id, module).is_some()
        {
            return Err(RegistryError::Graph(format!(
                "duplicate or wrongly layered module {}",
                module.id
            )));
        }
        for service in module.services {
            if !service.id.starts_with(&format!("{}.", module.id)) || !services.insert(service.id) {
                return Err(RegistryError::Graph(format!(
                    "invalid or duplicate service {}",
                    service.id
                )));
            }
        }
        for (version, _) in module.migrations {
            if !migrations.insert(version) {
                return Err(RegistryError::Graph(format!(
                    "duplicate migration {version}"
                )));
            }
        }
    }
    for module in pending.values() {
        for dependency in module.dependencies {
            let parent = pending.get(dependency).ok_or_else(|| {
                RegistryError::Graph(format!("{} requires missing {dependency}", module.id))
            })?;
            if parent.layer > module.layer {
                return Err(RegistryError::Graph(format!(
                    "{} depends upward on {dependency}",
                    module.id
                )));
            }
        }
    }
    let mut ordered = Vec::new();
    let mut installed = BTreeSet::new();
    while !pending.is_empty() {
        let next = pending
            .values()
            .filter(|module| module.dependencies.iter().all(|id| installed.contains(id)))
            .min_by_key(|module| (module.layer, module.id))
            .copied()
            .ok_or_else(|| RegistryError::Graph("dependency cycle".into()))?;
        pending.remove(next.id);
        installed.insert(next.id);
        ordered.push(next);
    }
    Ok(ordered)
}

#[derive(Default)]
pub struct Services {
    values: BTreeMap<&'static str, Value>,
    resolving: Option<BTreeSet<&'static str>>,
    catalog: Option<Catalog>,
}
impl Services {
    pub fn provide(&mut self, id: &'static str, value: Value) -> Result<()> {
        if self.values.contains_key(id) {
            return Err(RegistryError::Service(format!("duplicate {id}")));
        }
        self.values.insert(id, value);
        Ok(())
    }
    pub fn get<T: Any + Send + Sync>(&self, id: &str) -> Result<Arc<T>> {
        if let Some(allowed) = &self.resolving {
            let owner = id.rsplit_once('.').map(|(owner, _)| owner);
            if !id.starts_with("bootstrap.") && !owner.is_some_and(|owner| allowed.contains(owner))
            {
                return Err(RegistryError::Service(format!(
                    "undeclared dependency {id}"
                )));
            }
        }
        self.values
            .get(id)
            .cloned()
            .and_then(|value| value.downcast().ok())
            .ok_or_else(|| RegistryError::Service(id.into()))
    }
    pub fn catalog(&self) -> Result<&Catalog> {
        self.catalog
            .as_ref()
            .ok_or_else(|| RegistryError::Contribution("catalog not installed".into()))
    }
    pub fn install(&mut self, modules: &[&'static Module]) -> Result<()> {
        let modules = order(modules.to_vec())?;
        let catalog = Catalog::discover(&modules)?;
        for module in &modules {
            let mut allowed: BTreeSet<_> = module.dependencies.iter().copied().collect();
            allowed.insert(module.id);
            self.resolving = Some(allowed);
            for service in module.services {
                let value = (service.factory)(self);
                match value {
                    Ok(value) => self.provide(service.id, value)?,
                    Err(error) => {
                        self.resolving = None;
                        return Err(error);
                    }
                }
            }
        }
        self.resolving = None;
        for ready in inventory::iter::<Ready> {
            if !modules.iter().any(|module| module.id == ready.module) {
                return Err(RegistryError::Graph(format!(
                    "unknown readiness owner {}",
                    ready.module
                )));
            }
            (ready.check)(self)?;
        }
        self.catalog = Some(catalog);
        Ok(())
    }
}

/// A Base-owned boundary may be bound by an upstream adapter during installation.
/// Readiness checks run after all factories and must refuse missing adapters.
pub struct Ready {
    pub module: &'static str,
    pub check: fn(&Services) -> Result<()>,
}
inventory::collect!(Ready);

inventory::submit! {
    Module {
        id:"base/module_registry",layer:Layer::Base,dependencies:&[],migrations:&[],services:&[],
    }
}
