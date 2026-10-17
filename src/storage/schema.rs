// @generated automatically by Diesel CLI.

diesel::table! {
    clock_snapshots (game_id, node_id) {
        game_id -> Text,
        node_id -> Text,
        body -> Text,
    }
}

diesel::table! {
    documents (id) {
        id -> Text,
        revision -> Text,
        body -> Text,
    }
}

diesel::table! {
    editor_drafts (draft_id) {
        draft_id -> Text,
        storage_revision -> Text,
        schema_version -> Integer,
        body -> Nullable<Text>,
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
        date_year -> Nullable<Integer>,
        date_month -> Nullable<Integer>,
        date_day -> Nullable<Integer>,
        opening_display -> Nullable<Text>,
        opening_key -> Nullable<Text>,
        opening_code -> Nullable<Text>,
        plies -> Integer,
        identity_initial_fen -> Text,
        identity_mainline_uci -> Text,
        content_sha256 -> Binary,
        modified_ms -> Nullable<Integer>,
        favorite -> Integer,
        folder_id -> Nullable<Text>,
        location -> Text,
        trashed_ms -> Nullable<Integer>,
    }
}

diesel::table! {
    library_entry_tags (game_id, tag_id) {
        game_id -> Text,
        tag_id -> Text,
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
    library_state (singleton) {
        singleton -> Nullable<Integer>,
        revision -> Text,
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
    move_nodes (node_id) {
        node_id -> Text,
        game_id -> Text,
        parent_id -> Nullable<Text>,
        ordinal -> Integer,
        uci -> Nullable<Text>,
        san -> Nullable<Text>,
        fen -> Text,
        position_hash -> Binary,
        position_identity -> Text,
        annotations -> Text,
    }
}

diesel::table! {
    preferences (singleton) {
        singleton -> Nullable<Integer>,
        schema_version -> Integer,
        revision -> Text,
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

diesel::table! {
    puzzle_checkpoints (run_id) {
        run_id -> Text,
        body -> Text,
    }
}

diesel::table! {
    puzzle_collections (collection_id) {
        collection_id -> Text,
        schema_version -> Integer,
        digest -> Binary,
        body -> Text,
    }
}

diesel::table! {
    puzzle_events (run_id, ordinal) {
        run_id -> Text,
        ordinal -> Integer,
        body -> Text,
    }
}

diesel::table! {
    puzzle_runs (run_id) {
        run_id -> Text,
        collection_id -> Text,
        storage_revision -> Text,
        event_count -> Integer,
        journal_version -> Integer,
        journal_digest -> Binary,
    }
}

diesel::joinable!(games -> documents (game_id));
diesel::joinable!(library_entries -> documents (game_id));
diesel::joinable!(library_entries -> library_folders (folder_id));
diesel::joinable!(library_entry_tags -> library_entries (game_id));
diesel::joinable!(library_entry_tags -> library_tags (tag_id));
diesel::joinable!(puzzle_attempts -> puzzle_runs (run_id));
diesel::joinable!(puzzle_checkpoints -> puzzle_runs (run_id));
diesel::joinable!(puzzle_events -> puzzle_runs (run_id));
diesel::joinable!(puzzle_runs -> puzzle_collections (collection_id));

diesel::allow_tables_to_appear_in_same_query!(
    clock_snapshots,
    documents,
    editor_drafts,
    games,
    library_entries,
    library_entry_tags,
    library_folders,
    library_state,
    library_tags,
    move_nodes,
    preferences,
    puzzle_attempts,
    puzzle_checkpoints,
    puzzle_collections,
    puzzle_events,
    puzzle_runs,
);
