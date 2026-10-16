use std::collections::{BTreeMap, btree_map::Entry};

use thiserror::Error;

use crate::engine::enums::{EngineEnumOption, EngineOptionEnumDescriptor};
use crate::engine::package::manifest::EnginePackageManifest;

#[derive(Debug, Error)]
pub enum EngineCapabilitiesError {
    #[error("The engine identity does not match the packaged Reckless version")]
    IdentityMismatch,
    #[error("The engine advertised a malformed, duplicate, or incompatible option")]
    OptionMismatch,
    #[error("Engine option {name} with descriptor {descriptor} has no capability coverage")]
    UnknownOption { name: String, descriptor: String },
    #[error("The engine did not advertise every required capability")]
    MissingOptions,
}

#[derive(Debug, Clone, Default)]
pub struct EngineCapabilities {
    options: BTreeMap<EngineEnumOption, EngineOptionEnumDescriptor>,
}

impl EngineCapabilities {
    pub fn discover(&mut self, line: &str) -> Result<(), EngineCapabilitiesError> {
        let (name, descriptor) = line
            .strip_prefix("option name ")
            .and_then(|option| option.split_once(" type "))
            .ok_or(EngineCapabilitiesError::OptionMismatch)?;
        let option = EngineEnumOption::ALL
            .into_iter()
            .find(|option| option.name() == name)
            .ok_or_else(|| EngineCapabilitiesError::UnknownOption {
                name: name.to_owned(),
                descriptor: descriptor.to_owned(),
            })?;
        let descriptor = option
            .parse_descriptor(descriptor)
            .ok_or(EngineCapabilitiesError::OptionMismatch)?;
        match self.options.entry(option) {
            Entry::Vacant(entry) => {
                entry.insert(descriptor);
                Ok(())
            }
            Entry::Occupied(_) => Err(EngineCapabilitiesError::OptionMismatch),
        }
    }

    pub fn validate(&self) -> Result<(), EngineCapabilitiesError> {
        if self.options.len() != EngineEnumOption::ALL.len() {
            return Err(EngineCapabilitiesError::MissingOptions);
        }
        Ok(())
    }

    pub fn options(&self) -> impl ExactSizeIterator<Item = EngineEnumOption> + '_ {
        self.options.keys().copied()
    }

    pub fn descriptor(&self, option: EngineEnumOption) -> Option<EngineOptionEnumDescriptor> {
        self.options.get(&option).copied()
    }
}

#[derive(Debug, Clone)]
pub struct EngineIdentity {
    name: String,
    author: String,
    capabilities: EngineCapabilities,
}

impl EngineIdentity {
    pub const AUTHOR: &str = "Arseniy Surkov, Shahin M. Shahin, and Styx";

    pub fn new(
        name: String,
        author: String,
        capabilities: EngineCapabilities,
    ) -> Result<Self, EngineCapabilitiesError> {
        if name != format!("Reckless {}", EnginePackageManifest::VERSION) || author != Self::AUTHOR
        {
            return Err(EngineCapabilitiesError::IdentityMismatch);
        }
        capabilities.validate()?;
        Ok(Self {
            name,
            author,
            capabilities,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn author(&self) -> &str {
        &self.author
    }

    pub fn capabilities(&self) -> &EngineCapabilities {
        &self.capabilities
    }
}
