use semver::Version;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum UpdateReleaseCheckError {
    #[error("The tag {0:?} is not vMAJOR.MINOR.PATCH")]
    Tag(String),
    #[error("The {what} version {found} differs from the tag version {expected}")]
    Mismatch {
        what: &'static str,
        expected: String,
        found: String,
    },
    #[error("Publication is refused: {0}")]
    Publication(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdateReleaseState {
    pub draft: bool,
    pub assets_expected: usize,
    pub assets_uploaded: usize,
    pub all_verified: bool,
    pub allowance_remaining: bool,
}

pub struct UpdateReleaseCheck;

impl UpdateReleaseCheck {
    pub fn tag_version(tag: &str) -> Result<Version, UpdateReleaseCheckError> {
        let invalid = || UpdateReleaseCheckError::Tag(tag.to_owned());
        let text = tag.strip_prefix('v').ok_or_else(invalid)?;
        let version = Version::parse(text).map_err(|_error| invalid())?;
        if !version.pre.is_empty() || !version.build.is_empty() || version.to_string() != text {
            return Err(invalid());
        }
        Ok(version)
    }

    pub fn versions_match(
        tag: &str,
        cargo: &str,
        package: &str,
        manifest: &str,
    ) -> Result<(), UpdateReleaseCheckError> {
        let expected = Self::tag_version(tag)?.to_string();
        for (what, found) in [
            ("Cargo", cargo),
            ("package", package),
            ("manifest", manifest),
        ] {
            if found != expected {
                return Err(UpdateReleaseCheckError::Mismatch {
                    what,
                    expected,
                    found: found.to_owned(),
                });
            }
        }
        Ok(())
    }

    pub fn publication_allowed(state: UpdateReleaseState) -> Result<(), UpdateReleaseCheckError> {
        if !state.draft {
            return Err(UpdateReleaseCheckError::Publication(
                "the release must stay a draft until complete",
            ));
        }
        if state.assets_expected == 0 || state.assets_uploaded != state.assets_expected {
            return Err(UpdateReleaseCheckError::Publication(
                "not every expected asset is uploaded",
            ));
        }
        if !state.all_verified {
            return Err(UpdateReleaseCheckError::Publication(
                "an asset is not verified against the signed manifest",
            ));
        }
        if !state.allowance_remaining {
            return Err(UpdateReleaseCheckError::Publication(
                "the included allowance is exhausted and paid work must not start",
            ));
        }
        Ok(())
    }
}
