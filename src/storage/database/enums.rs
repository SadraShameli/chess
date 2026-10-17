#[derive(Clone, Copy)]
pub(crate) enum StorageDatabaseEnumIntegerProperty {
    UserVersion,
    PageSize,
    PageCount,
    AutoVacuum,
    FreelistCount,
    MaxPageCount,
}

impl StorageDatabaseEnumIntegerProperty {
    pub(super) const fn expression(self) -> &'static str {
        match self {
            Self::UserVersion => "(SELECT user_version FROM pragma_user_version)",
            Self::PageSize => "(SELECT page_size FROM pragma_page_size)",
            Self::PageCount => "(SELECT page_count FROM pragma_page_count)",
            Self::AutoVacuum => "(SELECT auto_vacuum FROM pragma_auto_vacuum)",
            Self::FreelistCount => "(SELECT freelist_count FROM pragma_freelist_count)",
            Self::MaxPageCount => "(SELECT max_page_count FROM pragma_max_page_count)",
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum StorageDatabaseEnumTextProperty {
    JournalMode,
    Encoding,
    MainPath,
}

impl StorageDatabaseEnumTextProperty {
    pub(super) const fn expression(self) -> &'static str {
        match self {
            Self::JournalMode => "(SELECT journal_mode FROM pragma_journal_mode)",
            Self::Encoding => "(SELECT encoding FROM pragma_encoding)",
            Self::MainPath => "(SELECT file FROM pragma_database_list WHERE name = 'main')",
        }
    }
}
