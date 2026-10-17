use std::fmt::{Display, Formatter, Write};
use std::marker::PhantomData;

use serde::de::{DeserializeOwned, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::value::RawValue;
use thiserror::Error;

use crate::engine::capabilities::{
    EngineCapabilities, EngineCapabilitiesError, EngineCapabilitiesVersion, EngineIdentity,
};
use crate::engine::enums::EngineEnumOption;

#[derive(Debug, Error)]
pub enum EnginePersistenceError {
    #[error("The historical engine record exceeds a byte or count bound")]
    Bounds,
    #[error("Historical engine record version {0} is unsupported")]
    Version(u32),
    #[error("The historical engine record contains invalid context")]
    Context,
    #[error("The historical analysis score perspective does not match its position")]
    Perspective,
    #[error("Historical analysis elapsed time must be exact whole milliseconds")]
    FractionalMilliseconds,
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Capabilities(#[from] EngineCapabilitiesError),
    #[error(transparent)]
    Options(#[from] crate::engine::options::EngineOptionsError),
    #[error(transparent)]
    Resources(#[from] crate::engine::resource_config::EngineResourceConfigError),
    #[error(transparent)]
    Analysis(#[from] crate::engine::analysis::EngineAnalysisError),
    #[error(transparent)]
    SearchIdentity(#[from] crate::engine::search::identity::EngineSearchIdentityError),
    #[error(transparent)]
    SearchLimit(#[from] crate::engine::search::limit::EngineSearchLimitError),
    #[error(transparent)]
    Duration(#[from] crate::clock::duration::ClockDurationError),
    #[error(transparent)]
    Conversion(#[from] std::num::TryFromIntError),
}

#[derive(Serialize)]
struct EngineRecordEnvelopeRef<'a, T> {
    version: u32,
    data: &'a T,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EngineRecordEnvelope<'a> {
    version: u32,
    #[serde(borrow)]
    data: &'a RawValue,
}

pub(crate) struct EngineRecordJson;

impl EngineRecordJson {
    const VERSION: u32 = 1;

    pub(crate) fn serialize<T: Serialize, S: Serializer>(
        data: &T,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        EngineRecordEnvelopeRef {
            version: Self::VERSION,
            data,
        }
        .serialize(serializer)
    }

    pub(crate) fn deserialize<
        'de,
        D: Deserializer<'de>,
        T: DeserializeOwned,
        const MAX_BYTES: usize,
    >(
        deserializer: D,
    ) -> Result<T, D::Error> {
        let raw = <&RawValue>::deserialize(deserializer)?;
        if raw.get().len() > MAX_BYTES {
            return Err(serde::de::Error::custom(EnginePersistenceError::Bounds));
        }
        let envelope: EngineRecordEnvelope<'_> =
            serde_json::from_str(raw.get()).map_err(serde::de::Error::custom)?;
        if envelope.version != Self::VERSION {
            return Err(serde::de::Error::custom(EnginePersistenceError::Version(
                envelope.version,
            )));
        }
        serde_json::from_str(envelope.data.get()).map_err(serde::de::Error::custom)
    }

    pub(crate) fn parse<T: DeserializeOwned, const MAX_BYTES: usize>(
        bytes: &[u8],
    ) -> Result<T, EnginePersistenceError> {
        if bytes.len() > MAX_BYTES {
            return Err(EnginePersistenceError::Bounds);
        }
        Ok(serde_json::from_slice(bytes)?)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct EngineRecordText<const MAX_BYTES: usize>(String);

impl<const MAX_BYTES: usize> EngineRecordText<MAX_BYTES> {
    pub(crate) fn new(text: &str) -> Result<Self, EnginePersistenceError> {
        if text.len() > MAX_BYTES {
            return Err(EnginePersistenceError::Bounds);
        }
        Ok(Self(text.to_owned()))
    }

    pub(crate) fn display(value: &impl Display) -> Result<Self, EnginePersistenceError> {
        let mut text = Self(String::new());
        write!(&mut text, "{value}").map_err(|_error| EnginePersistenceError::Bounds)?;
        Ok(text)
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn into_string(self) -> String {
        self.0
    }
}

impl<const MAX_BYTES: usize> Write for EngineRecordText<MAX_BYTES> {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        if self
            .0
            .len()
            .checked_add(text.len())
            .is_none_or(|bytes| bytes > MAX_BYTES)
        {
            return Err(std::fmt::Error);
        }
        self.0.push_str(text);
        Ok(())
    }
}

struct EngineRecordTextVisitor<const MAX_BYTES: usize>;

impl<const MAX_BYTES: usize> Visitor<'_> for EngineRecordTextVisitor<MAX_BYTES> {
    type Value = EngineRecordText<MAX_BYTES>;

    fn expecting(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "text with at most {MAX_BYTES} bytes")
    }

    fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<Self::Value, E> {
        EngineRecordText::new(text).map_err(E::custom)
    }
}

impl<'de, const MAX_BYTES: usize> Deserialize<'de> for EngineRecordText<MAX_BYTES> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(EngineRecordTextVisitor)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct EngineRecordList<T, const MAX_COUNT: usize>(Vec<T>);

impl<T, const MAX_COUNT: usize> EngineRecordList<T, MAX_COUNT> {
    pub(crate) fn new() -> Self {
        Self(Vec::new())
    }

    pub(crate) fn push(&mut self, value: T) -> Result<(), EnginePersistenceError> {
        if self.0.len() >= MAX_COUNT {
            return Err(EnginePersistenceError::Bounds);
        }
        self.0.push(value);
        Ok(())
    }

    pub(crate) fn as_slice(&self) -> &[T] {
        &self.0
    }
}

struct EngineRecordListVisitor<T, const MAX_COUNT: usize>(PhantomData<T>);

impl<'de, T: Deserialize<'de>, const MAX_COUNT: usize> Visitor<'de>
    for EngineRecordListVisitor<T, MAX_COUNT>
{
    type Value = EngineRecordList<T, MAX_COUNT>;

    fn expecting(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "at most {MAX_COUNT} entries")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
        if sequence.size_hint().is_some_and(|count| count > MAX_COUNT) {
            return Err(serde::de::Error::custom(EnginePersistenceError::Bounds));
        }
        let mut values = EngineRecordList::new();
        while values.as_slice().len() < MAX_COUNT {
            let Some(value) = sequence.next_element()? else {
                return Ok(values);
            };
            values.push(value).map_err(serde::de::Error::custom)?;
        }
        if sequence.next_element::<serde::de::IgnoredAny>()?.is_some() {
            return Err(serde::de::Error::custom(EnginePersistenceError::Bounds));
        }
        Ok(values)
    }
}

impl<'de, T: Deserialize<'de>, const MAX_COUNT: usize> Deserialize<'de>
    for EngineRecordList<T, MAX_COUNT>
{
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_seq(EngineRecordListVisitor(PhantomData))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EngineIdentityRecordData {
    registry_version: u32,
    name: EngineRecordText<256>,
    author: EngineRecordText<256>,
    descriptors: EngineRecordList<EngineRecordText<256>, { EngineEnumOption::ALL.len() }>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineIdentityRecord {
    data: EngineIdentityRecordData,
}

impl EngineIdentityRecord {
    pub const MAX_BYTES: usize = 16 * 1024;

    pub fn capture(identity: &EngineIdentity) -> Result<Self, EnginePersistenceError> {
        identity.capabilities().validate()?;
        let mut descriptors = EngineRecordList::new();
        for option in EngineEnumOption::ALL {
            let descriptor = identity
                .capabilities()
                .descriptor(option)
                .ok_or(EngineCapabilitiesError::MissingOptions)?;
            descriptors.push(EngineRecordText::display(&format_args!(
                "option name {} type {}",
                option.name(),
                descriptor.descriptor()
            ))?)?;
        }
        Ok(Self {
            data: EngineIdentityRecordData {
                registry_version: identity.capabilities().version().get(),
                name: EngineRecordText::new(identity.name())?,
                author: EngineRecordText::new(identity.author())?,
                descriptors,
            },
        })
    }

    pub fn restore(&self) -> Result<EngineIdentity, EnginePersistenceError> {
        EngineCapabilitiesVersion::try_from(self.data.registry_version)?;
        let mut capabilities = EngineCapabilities::default();
        for descriptor in self.data.descriptors.as_slice() {
            capabilities.discover(descriptor.as_str())?;
        }
        capabilities.validate()?;
        Ok(EngineIdentity::new(
            self.data.name.as_str().to_owned(),
            self.data.author.as_str().to_owned(),
            capabilities,
        )?)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, EnginePersistenceError> {
        EngineRecordJson::parse::<Self, { Self::MAX_BYTES }>(bytes)
    }
}

impl Serialize for EngineIdentityRecord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        EngineRecordJson::serialize(&self.data, serializer)
    }
}

impl<'de> Deserialize<'de> for EngineIdentityRecord {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let record = Self {
            data: EngineRecordJson::deserialize::<D, EngineIdentityRecordData, { Self::MAX_BYTES }>(
                deserializer,
            )?,
        };
        record.restore().map_err(serde::de::Error::custom)?;
        Ok(record)
    }
}
