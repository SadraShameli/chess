use thiserror::Error;

#[derive(Debug, Error)]
pub enum UpdateVerifierError {
    #[error("No update public key is embedded in this build")]
    NotProvisioned,
    #[error("The signature exceeds {0} bytes")]
    SignatureTooLarge(usize),
    #[error("The signature is not valid text")]
    SignatureEncoding,
    #[error("The public key or signature is malformed: {0}")]
    Malformed(String),
    #[error("The signature does not match the signed bytes")]
    Mismatch,
}

pub trait UpdateSignatureVerifier {
    fn verify(&self, signed: &[u8], signature: &[u8]) -> Result<(), UpdateVerifierError>;
}

pub struct UpdateVerifier {
    key: minisign_verify::PublicKey,
}

impl UpdateVerifier {
    pub const MAX_SIGNATURE_BYTES: usize = 16 * 1024;
    pub const EMBEDDED_PUBLIC_KEY: Option<&'static str> = None;

    pub fn new(public_key: &str) -> Result<Self, UpdateVerifierError> {
        let key = minisign_verify::PublicKey::from_base64(public_key.trim())
            .map_err(|error| UpdateVerifierError::Malformed(error.to_string()))?;
        Ok(Self { key })
    }

    pub fn embedded() -> Result<Self, UpdateVerifierError> {
        Self::new(Self::EMBEDDED_PUBLIC_KEY.ok_or(UpdateVerifierError::NotProvisioned)?)
    }
}

impl UpdateSignatureVerifier for UpdateVerifier {
    fn verify(&self, signed: &[u8], signature: &[u8]) -> Result<(), UpdateVerifierError> {
        if signature.len() > Self::MAX_SIGNATURE_BYTES {
            return Err(UpdateVerifierError::SignatureTooLarge(
                Self::MAX_SIGNATURE_BYTES,
            ));
        }
        let text = std::str::from_utf8(signature)
            .map_err(|_error| UpdateVerifierError::SignatureEncoding)?;
        let decoded = minisign_verify::Signature::decode(text)
            .map_err(|error| UpdateVerifierError::Malformed(error.to_string()))?;
        self.key
            .verify(signed, &decoded, false)
            .map_err(|_error| UpdateVerifierError::Mismatch)
    }
}
