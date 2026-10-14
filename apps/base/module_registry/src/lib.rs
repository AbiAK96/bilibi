//! Deep modules submit descriptors at link time. Discovery validates the graph,
//! sorts it dependency-first, and builds named typed services without host lists.
use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
pub type Value = Arc<dyn Any + Send + Sync>;
pub type Result<T> = std::result::Result<T, RegistryError>;
pub type Factory = fn(&Services) -> Result<Value>;
pub type Migration = (&'static str, &'static str);

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("Invalid module graph: {0}")]
    Graph(String),
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
    order(inventory::iter::<Module>.into_iter().collect())
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
        self.values
            .get(id)
            .cloned()
            .and_then(|value| value.downcast().ok())
            .ok_or_else(|| RegistryError::Service(id.into()))
    }
    pub fn install(&mut self, modules: &[&Module]) -> Result<()> {
        for module in modules {
            for service in module.services {
                self.provide(service.id, (service.factory)(self)?)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    static BASE: Module = Module {
        id: "base/a",
        layer: Layer::Base,
        dependencies: &[],
        migrations: &[],
        services: &[],
    };
    static CORE: Module = Module {
        id: "core/b",
        layer: Layer::Core,
        dependencies: &["base/a"],
        migrations: &[],
        services: &[],
    };
    static DOMAIN: Module = Module {
        id: "domains/c",
        layer: Layer::Domains,
        dependencies: &["core/b"],
        migrations: &[],
        services: &[],
    };
    static EXT: Module = Module {
        id: "extensions/d",
        layer: Layer::Extensions,
        dependencies: &["domains/c"],
        migrations: &[],
        services: &[],
    };
    #[test]
    fn dependency_order_covers_all_four_roots() {
        let result = order(vec![&EXT, &CORE, &DOMAIN, &BASE]).unwrap();
        assert_eq!(
            result.iter().map(|module| module.id).collect::<Vec<_>>(),
            vec!["base/a", "core/b", "domains/c", "extensions/d"]
        );
        assert!(order(vec![&CORE]).is_err());
        assert!(order(vec![&BASE, &BASE]).is_err());
    }
    #[test]
    fn rejects_cycles_upward_dependencies_and_wrong_types() {
        static A: Module = Module {
            id: "base/a",
            layer: Layer::Base,
            dependencies: &["base/b"],
            migrations: &[],
            services: &[],
        };
        static B: Module = Module {
            id: "base/b",
            layer: Layer::Base,
            dependencies: &["base/a"],
            migrations: &[],
            services: &[],
        };
        static UP: Module = Module {
            id: "base/up",
            layer: Layer::Base,
            dependencies: &["core/b"],
            migrations: &[],
            services: &[],
        };
        assert!(order(vec![&A, &B]).is_err());
        assert!(order(vec![&BASE, &CORE, &UP]).is_err());
        let mut services = Services::default();
        services.provide("number", Arc::new(42_u32)).unwrap();
        assert_eq!(*services.get::<u32>("number").unwrap(), 42);
        assert!(services.get::<String>("number").is_err());
    }
}

inventory::submit! {
    Module {
        id:"base/module_registry",layer:Layer::Base,dependencies:&[],migrations:&[],services:&[],
    }
}
