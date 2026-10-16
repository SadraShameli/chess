use schemars::{JsonSchema, Schema, generate::SchemaSettings};

use crate::desktop::dto::{
    DesktopBootstrapSnapshot, DesktopEnumEvent, DesktopEnumRequest, DesktopEnumResponseResult,
    DesktopEnumValue, DesktopResponse,
};
use crate::desktop::envelope::DesktopEnvelope;
use crate::desktop::stream::DesktopStreamEvent;

#[derive(JsonSchema, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopSchema {
    pub request: DesktopEnvelope<DesktopEnumRequest>,
    pub response: DesktopResponse,
    pub event: DesktopStreamEvent<DesktopEnumEvent>,
}

impl DesktopSchema {
    pub const ID: &str = "urn:reckless-chess:desktop:v1";

    pub fn generate() -> Schema {
        let mut settings = SchemaSettings::draft07().for_serialize();
        settings.inline_subschemas = true;
        let mut schema = settings.into_generator().into_root_schema_for::<Self>();
        schema.insert("$id".to_owned(), Self::ID.into());
        schema
    }

    pub fn fixture() -> Result<Self, crate::desktop::error::DesktopError> {
        let request_id = "123e4567-e89b-42d3-a456-426614174000".parse()?;
        let board = crate::board::position::BoardPosition::standard();
        let snapshot = DesktopBootstrapSnapshot::new(&board, std::num::NonZeroU32::MIN);
        let now = crate::clock::duration::ClockInstant::from_nanos(0);
        let mut stream = crate::desktop::stream::DesktopStream::new(request_id, now);
        let event = stream
            .publish(
                DesktopEnumEvent::Snapshot(snapshot.clone()),
                crate::desktop::stream::DesktopEnumStreamClass::Snapshot,
                now,
            )?
            .ok_or(crate::desktop::error::DesktopError::ResyncRequired)?;
        Ok(Self {
            request: DesktopEnvelope::new(
                request_id,
                Some(u64::MAX.to_string().parse()?),
                DesktopEnumRequest::Bootstrap,
            ),
            response: DesktopResponse::new(
                request_id,
                DesktopEnumResponseResult::Success {
                    value: DesktopEnumValue::Bootstrap(snapshot),
                    revision: None,
                },
            ),
            event,
        })
    }
}
