diesel::table! {
    documents (id) {
        id -> Text,
        revision -> Text,
        body -> Text,
    }
}

diesel::table! {
    games (game_id) {
        game_id -> Text,
        variant -> Text,
        initial_fen -> Text,
        mode -> Text,
        phase -> Text,
        live_node_id -> Nullable<Text>,
        view_node_id -> Nullable<Text>,
        metadata -> Text,
    }
}

diesel::table! {
    library_state (singleton) {
        singleton -> BigInt,
        revision -> Text,
    }
}

diesel::table! {
    library_folders (folder_id) {
        folder_id -> Text,
        parent_id -> Nullable<Text>,
        revision -> Text,
        name -> Text,
    }
}

diesel::table! {
    library_tags (tag_id) {
        tag_id -> Text,
        revision -> Text,
        name -> Text,
    }
}

diesel::table! {
    library_entries (game_id) {
        game_id -> Text,
        content_revision -> Text,
        organization_revision -> Text,
        content_kind -> Text,
        rules_variant -> Text,
        start_kind -> Text,
        mode -> Nullable<Text>,
        result -> Text,
        white_display -> Nullable<Text>,
        white_key -> Nullable<Text>,
        black_display -> Nullable<Text>,
        black_key -> Nullable<Text>,
        date_year -> Nullable<BigInt>,
        date_month -> Nullable<BigInt>,
        date_day -> Nullable<BigInt>,
        opening_display -> Nullable<Text>,
        opening_key -> Nullable<Text>,
        opening_code -> Nullable<Text>,
        plies -> BigInt,
        identity_initial_fen -> Text,
        identity_mainline_uci -> Text,
        content_sha256 -> Binary,
        modified_ms -> Nullable<BigInt>,
        favorite -> BigInt,
        folder_id -> Nullable<Text>,
        location -> Text,
        trashed_ms -> Nullable<BigInt>,
    }
}

diesel::table! {
    library_entry_tags (game_id, tag_id) {
        game_id -> Text,
        tag_id -> Text,
    }
}

diesel::table! {
    puzzle_collections (collection_id) {
        collection_id -> Text,
        schema_version -> BigInt,
        digest -> Binary,
        body -> Text,
    }
}

diesel::table! {
    puzzle_runs (run_id) {
        run_id -> Text,
        collection_id -> Text,
        storage_revision -> Text,
        event_count -> BigInt,
        journal_version -> BigInt,
        journal_digest -> Binary,
    }
}

diesel::table! {
    puzzle_events (run_id, ordinal) {
        run_id -> Text,
        ordinal -> BigInt,
        body -> Text,
    }
}

diesel::table! {
    puzzle_checkpoints (run_id) {
        run_id -> Text,
        body -> Text,
    }
}

diesel::table! {
    puzzle_attempts (run_id, attempt_id) {
        run_id -> Text,
        attempt_id -> Text,
        body -> Text,
    }
}

diesel::allow_tables_to_appear_in_same_query!(
    documents,
    games,
    library_state,
    library_folders,
    library_tags,
    library_entries,
    library_entry_tags,
    puzzle_collections,
    puzzle_runs,
    puzzle_events,
    puzzle_checkpoints,
    puzzle_attempts,
);
