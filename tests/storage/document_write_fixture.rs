#[derive(Clone, Copy)]
pub(crate) enum StorageDocumentWriteFixtureEnumOperation {
    Insert,
    Update,
}

impl StorageDocumentWriteFixtureEnumOperation {
    pub(crate) const ALL: [Self; 2] = [Self::Insert, Self::Update];

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Insert => "insert",
            Self::Update => "update",
        }
    }

    pub(crate) const fn sql(self) -> &'static str {
        match self {
            Self::Insert => "INSERT",
            Self::Update => "UPDATE OF body",
        }
    }
}
