use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::engine::capabilities::persistence::{EnginePersistenceError, EngineRecordJson};
use crate::engine::package::manifest::{EnginePackageEnumProfile, EnginePackageEnumTarget};
use crate::engine::package::selector::EnginePackageSelected;
use crate::package::digest::PackageDigest;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EnginePackageIdentityData {
    target: EnginePackageEnumTarget,
    profile: EnginePackageEnumProfile,
    digest: PackageDigest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnginePackageIdentity {
    data: EnginePackageIdentityData,
}

impl EnginePackageIdentity {
    pub const MAX_BYTES: usize = 1024;

    pub const fn new(
        target: EnginePackageEnumTarget,
        profile: EnginePackageEnumProfile,
        digest: PackageDigest,
    ) -> Self {
        Self {
            data: EnginePackageIdentityData {
                target,
                profile,
                digest,
            },
        }
    }

    pub const fn target(self) -> EnginePackageEnumTarget {
        self.data.target
    }

    pub const fn profile(self) -> EnginePackageEnumProfile {
        self.data.profile
    }

    pub const fn digest(self) -> PackageDigest {
        self.data.digest
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, EnginePersistenceError> {
        EngineRecordJson::parse::<Self, { Self::MAX_BYTES }>(bytes)
    }
}

impl EnginePackageSelected {
    pub fn identity(&self) -> EnginePackageIdentity {
        EnginePackageIdentity::new(self.target(), self.profile(), self.digest())
    }
}

impl Serialize for EnginePackageIdentity {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        EngineRecordJson::serialize(&self.data, serializer)
    }
}

impl<'de> Deserialize<'de> for EnginePackageIdentity {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self {
            data: EngineRecordJson::deserialize::<D, EnginePackageIdentityData, { Self::MAX_BYTES }>(
                deserializer,
            )?,
        })
    }
}
