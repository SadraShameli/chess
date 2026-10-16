use std::fmt::{Display, Formatter};
use std::io::Read;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EnginePackageDigestError {
    #[error("SHA-256 identities must contain exactly 64 lowercase hexadecimal characters")]
    Invalid,
    #[error("Cannot read the engine artifact: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct EnginePackageDigest([u8; 32]);

impl EnginePackageDigest {
    pub fn read(mut reader: impl Read) -> Result<Self, EnginePackageDigestError> {
        let mut digest = Sha256::new();
        let mut buffer = vec![0; 64 * 1024];
        loop {
            let count = reader.read(&mut buffer)?;
            if count == 0 {
                return Ok(Self(digest.finalize().into()));
            }
            digest.update(&buffer[..count]);
        }
    }
}

impl FromStr for EnginePackageDigest {
    type Err = EnginePackageDigestError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(EnginePackageDigestError::Invalid);
        }
        let mut digest = [0; 32];
        for (index, byte) in digest.iter_mut().enumerate() {
            let start = index * 2;
            *byte = u8::from_str_radix(&value[start..start + 2], 16)
                .map_err(|_error| EnginePackageDigestError::Invalid)?;
        }
        Ok(Self(digest))
    }
}

impl TryFrom<String> for EnginePackageDigest {
    type Error = EnginePackageDigestError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<EnginePackageDigest> for String {
    fn from(value: EnginePackageDigest) -> Self {
        value.to_string()
    }
}

impl Display for EnginePackageDigest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}
