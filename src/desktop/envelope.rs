use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};

use crate::desktop::error::DesktopError;
use crate::desktop::identity::{DesktopRequestId, DesktopRevision};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct DesktopProtocolVersion(u32);

impl DesktopProtocolVersion {
    pub const CURRENT: Self = Self(1);
}

impl TryFrom<u32> for DesktopProtocolVersion {
    type Error = DesktopError;
    fn try_from(received: u32) -> Result<Self, Self::Error> {
        if received == Self::CURRENT.0 {
            Ok(Self(received))
        } else {
            Err(DesktopError::UnsupportedProtocol { received })
        }
    }
}

impl From<DesktopProtocolVersion> for u32 {
    fn from(value: DesktopProtocolVersion) -> Self {
        value.0
    }
}

impl JsonSchema for DesktopProtocolVersion {
    fn schema_name() -> Cow<'static, str> {
        "DesktopProtocolVersion".into()
    }
    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "integer", "const": Self::CURRENT.0})
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DesktopEnvelope<T> {
    protocol_version: DesktopProtocolVersion,
    request_id: DesktopRequestId,
    expected_revision: Option<DesktopRevision>,
    payload: T,
}

impl<T> DesktopEnvelope<T> {
    pub fn new(
        request_id: DesktopRequestId,
        expected_revision: Option<DesktopRevision>,
        payload: T,
    ) -> Self {
        Self {
            protocol_version: DesktopProtocolVersion::CURRENT,
            request_id,
            expected_revision,
            payload,
        }
    }

    pub fn request_id(&self) -> DesktopRequestId {
        self.request_id
    }
    pub fn expected_revision(&self) -> Option<DesktopRevision> {
        self.expected_revision
    }
    pub fn payload(&self) -> &T {
        &self.payload
    }
    pub fn into_payload(self) -> T {
        self.payload
    }
}
