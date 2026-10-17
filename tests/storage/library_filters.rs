use std::collections::HashSet;

use chess::board::enums::{BoardEnumColor, BoardEnumVariant};
use chess::game::enums::{GameEnumMode, GameEnumResult};
use chess::game::library_organization::{
    GameLibraryOrganizationEnumMutation, GameLibraryOrganizationTimestamp,
};
use chess::game::session::{GameMetadataDate, GameMetadataKnownTags, GameMetadataOpeningCode};
use chess::notation::game_identity::NotationGameIdentityEnumResult;
use chess::storage::library_prepared::{
    StorageLibraryEnumContentChange, StorageLibraryEnumStartKind,
};
use chess::storage::library_query::{
    StorageLibraryDateInterval, StorageLibraryEnumFilter, StorageLibraryEnumFolderFilter,
    StorageLibraryEnumSort, StorageLibraryEnumTagFilter, StorageLibraryPlyInterval,
    StorageLibraryQuery, StorageLibraryQueryOptions,
};
use chess::storage::library_record::StorageLibraryEnumContent;
use chess::storage::library_repository::StorageLibraryError;

#[path = "library_filters_fixture.rs"]
mod fixture;

use crate::storage_library_filters::fixture::{
    StorageLibraryFiltersFixture, StorageLibraryFiltersNotation, StorageLibraryFiltersTestResult,
};

#[tokio::test]
async fn heterogeneous_keysets_match_fixed_groups_in_every_sort_direction()
-> StorageLibraryFiltersTestResult {
    let fixture = StorageLibraryFiltersFixture::new().await?;
    let counts = [13_usize, 123, 31, 29, 47, 17];
    let modified = [Some(0), Some(200), Some(900), Some(100), None, Some(300)];
    let dates = [
        Some(GameMetadataDate::new(Some(2023), Some(12), Some(31))?),
        Some(GameMetadataDate::new(Some(2024), Some(2), Some(29))?),
        Some(GameMetadataDate::new(Some(2025), Some(1), Some(1))?),
        Some(GameMetadataDate::new(Some(2024), None, Some(29))?),
        None,
        Some(GameMetadataDate::new(None, Some(2), Some(29))?),
    ];
    let plies = [0, 2, 6, 4, 1, 3];
    let mut groups = vec![Vec::new(); counts.len()];
    let largest_group = counts
        .iter()
        .copied()
        .max()
        .ok_or("missing keyset groups")?;
    for ordinal in (0..largest_group).rev() {
        for (group, count) in counts.iter().enumerate().rev() {
            if ordinal >= *count {
                continue;
            }
            let value = 10_000 + u128::try_from(ordinal)? * 6 + u128::try_from(group)?;
            let id = StorageLibraryFiltersFixture::id(value)?;
            let body = StorageLibraryFiltersFixture::notation_body(
                id,
                StorageLibraryFiltersNotation {
                    known: GameMetadataKnownTags {
                        date: dates[group],
                        white: Some(StorageLibraryFiltersFixture::text(&format!(
                            "Group {group}"
                        ))?),
                        ..GameMetadataKnownTags::default()
                    },
                    plies: plies[group],
                    ..StorageLibraryFiltersNotation::default()
                },
            )?;
            if let Some(timestamp) = modified[group] {
                fixture.insert(id, body, timestamp).await?;
            } else {
                fixture.adopt(id, body).await?;
            }
            groups[group].push(id);
        }
    }
    for group in &mut groups {
        group.sort_unstable_by_key(|id| id.as_uuid());
    }
    assert_eq!(counts.iter().sum::<usize>(), 260);
    assert_eq!(StorageLibraryQuery::PAGE_SIZE, 100);
    for (sort, order) in [
        (
            StorageLibraryEnumSort::UpdatedDescending,
            [2, 5, 1, 3, 0, 4],
        ),
        (StorageLibraryEnumSort::UpdatedAscending, [0, 3, 1, 5, 2, 4]),
        (StorageLibraryEnumSort::DateDescending, [2, 1, 0, 3, 5, 4]),
        (StorageLibraryEnumSort::DateAscending, [0, 1, 2, 4, 5, 3]),
        (StorageLibraryEnumSort::PliesDescending, [2, 3, 5, 1, 4, 0]),
        (StorageLibraryEnumSort::PliesAscending, [0, 4, 1, 5, 3, 2]),
    ] {
        let query = StorageLibraryQuery::new(StorageLibraryQueryOptions {
            sort,
            ..StorageLibraryQueryOptions::default()
        })?;
        let expected: Vec<_> = order
            .into_iter()
            .flat_map(|group| groups[group].iter().copied())
            .collect();
        let mut cursor = None;
        let mut actual = Vec::new();
        let mut page_lengths = Vec::new();
        let mut catalog = None;
        loop {
            let request = match cursor.take() {
                Some(cursor) => query.clone().with_cursor(cursor)?,
                None => query.clone(),
            };
            let page = fixture.repository().query(request).await?;
            assert_eq!(page.unindexed_document_count(), 0);
            if let Some(catalog) = catalog {
                assert_eq!(page.catalog_revision(), catalog);
            } else {
                catalog = Some(page.catalog_revision());
            }
            assert!(page.entries().len() <= 100);
            for entry in page.entries() {
                let group = usize::try_from((entry.id().as_uuid().as_u128() - 10_000) % 6)?;
                assert_eq!(
                    entry
                        .modified()
                        .map(GameLibraryOrganizationTimestamp::value),
                    modified[group]
                );
                assert_eq!(entry.date(), dates[group]);
                assert_eq!(entry.plies(), u32::try_from(plies[group])?);
                assert!(!entry.is_partial_date_match());
                actual.push(entry.id());
            }
            page_lengths.push(page.entries().len());
            cursor = page.next_cursor().cloned();
            if cursor.is_none() {
                break;
            }
            assert!(
                page_lengths.len() < 3,
                "continuation did not exhaust the fixture"
            );
        }
        assert_eq!(page_lengths, [100, 100, 60], "{sort:?}");
        assert_eq!(
            actual.iter().copied().collect::<HashSet<_>>().len(),
            260,
            "{sort:?}"
        );
        assert_eq!(actual, expected, "{sort:?}");
        assert_eq!(groups[1].len(), 123);
    }
    fixture.close().await
}

#[tokio::test]
async fn player_side_filters_normalize_unicode_and_preserve_display()
-> StorageLibraryFiltersTestResult {
    let fixture = StorageLibraryFiltersFixture::new().await?;
    fixture.representative_records().await?;
    for (white, black, either, ids) in [
        (Some("  e\u{301}LODIE \u{2003}"), None, None, vec![1]),
        (None, Some("Élodie"), None, vec![2]),
        (None, None, Some("éLODIE"), vec![1, 2]),
        (Some("STRASSE"), None, None, vec![5]),
        (None, Some("strasse"), None, vec![1]),
        (None, None, Some("STRASSE"), vec![1, 5]),
        (Some("LODI"), Some("STRASSE"), None, vec![1]),
        (Some("Bob"), Some("STRASSE"), None, vec![]),
    ] {
        let filter = |value: Option<&str>| -> StorageLibraryFiltersTestResult<_> {
            Ok(match value {
                Some(value) => {
                    StorageLibraryEnumFilter::Value(StorageLibraryFiltersFixture::text(value)?)
                }
                None => StorageLibraryEnumFilter::Any,
            })
        };
        fixture
            .expect_ids(
                StorageLibraryQueryOptions {
                    white: filter(white)?,
                    black: filter(black)?,
                    either_player: filter(either)?,
                    ..StorageLibraryQueryOptions::default()
                },
                &ids,
            )
            .await?;
    }
    let page = fixture
        .repository()
        .query(StorageLibraryQuery::default())
        .await?;
    let first = page
        .entries()
        .iter()
        .find(|entry| entry.id().as_uuid().as_u128() == 1)
        .ok_or("missing parsed player record")?;
    assert_eq!(first.white().map(|text| text.as_str()), Some("  Élodie  "));
    assert_eq!(first.black().map(|text| text.as_str()), Some("Straße"));
    let second = page
        .entries()
        .iter()
        .find(|entry| entry.id().as_uuid().as_u128() == 2)
        .ok_or("missing decomposed player record")?;
    assert_eq!(
        second.black().map(|text| text.as_str()),
        Some("  E\u{301}LODIE ")
    );
    drop(page);
    fixture.close().await
}

#[tokio::test]
async fn active_player_filters_reject_missing_searches_and_exclude_missing_names()
-> StorageLibraryFiltersTestResult {
    let fixture = StorageLibraryFiltersFixture::new().await?;
    fixture.representative_records().await?;
    for value in ["", " \u{2003} ", "?", " ? "] {
        let value = StorageLibraryEnumFilter::Value(StorageLibraryFiltersFixture::text(value)?);
        for options in [
            StorageLibraryQueryOptions {
                white: value.clone(),
                ..StorageLibraryQueryOptions::default()
            },
            StorageLibraryQueryOptions {
                black: value.clone(),
                ..StorageLibraryQueryOptions::default()
            },
            StorageLibraryQueryOptions {
                either_player: value,
                ..StorageLibraryQueryOptions::default()
            },
        ] {
            assert!(matches!(
                StorageLibraryQuery::new(options),
                Err(StorageLibraryError::EmptyFilter)
            ));
        }
    }
    fixture
        .expect_ids(
            StorageLibraryQueryOptions {
                either_player: StorageLibraryEnumFilter::Value(StorageLibraryFiltersFixture::text(
                    "not present",
                )?),
                ..StorageLibraryQueryOptions::default()
            },
            &[],
        )
        .await?;
    fixture
        .expect_ids(
            StorageLibraryQueryOptions::default(),
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
        )
        .await?;
    fixture.close().await
}

#[tokio::test]
async fn result_filters_keep_notation_results_and_played_outcomes_distinct()
-> StorageLibraryFiltersTestResult {
    let fixture = StorageLibraryFiltersFixture::new().await?;
    fixture.representative_records().await?;
    for (result, ids) in [
        (
            NotationGameIdentityEnumResult::Completed(GameEnumResult::Victory(
                BoardEnumColor::White,
            )),
            vec![1],
        ),
        (
            NotationGameIdentityEnumResult::Completed(GameEnumResult::Victory(
                BoardEnumColor::Black,
            )),
            vec![2, 6],
        ),
        (
            NotationGameIdentityEnumResult::Completed(GameEnumResult::Draw),
            vec![3],
        ),
        (
            NotationGameIdentityEnumResult::Unfinished,
            vec![4, 5, 7, 8, 9, 10],
        ),
    ] {
        fixture
            .expect_ids(
                StorageLibraryQueryOptions {
                    result: StorageLibraryEnumFilter::Value(result),
                    ..StorageLibraryQueryOptions::default()
                },
                &ids,
            )
            .await?;
    }
    let notation = fixture
        .repository()
        .load(StorageLibraryFiltersFixture::id(1)?)
        .await?
        .ok_or("missing notation envelope")?;
    assert!(matches!(
        notation.record().content(),
        StorageLibraryEnumContent::Notation(_)
    ));
    let played = fixture
        .repository()
        .load(StorageLibraryFiltersFixture::id(6)?)
        .await?
        .ok_or("missing played envelope")?;
    let StorageLibraryEnumContent::Played(session) = played.record().content() else {
        return Err("played record lost its session".into());
    };
    assert_eq!(
        session
            .outcome()
            .ok_or("missing played checkmate outcome")?
            .result(),
        GameEnumResult::Victory(BoardEnumColor::Black)
    );
    drop(notation);
    drop(played);
    fixture.close().await
}

#[tokio::test]
async fn rules_variant_and_start_provenance_are_independent_filters()
-> StorageLibraryFiltersTestResult {
    let fixture = StorageLibraryFiltersFixture::new().await?;
    fixture.representative_records().await?;
    for (variant, ids) in [
        (BoardEnumVariant::Standard, vec![1, 2, 3, 4, 5, 6, 9]),
        (BoardEnumVariant::Chess960, vec![7, 8, 10]),
    ] {
        fixture
            .expect_ids(
                StorageLibraryQueryOptions {
                    rules_variant: StorageLibraryEnumFilter::Value(variant),
                    ..StorageLibraryQueryOptions::default()
                },
                &ids,
            )
            .await?;
    }
    for (kind, ids) in [
        (
            StorageLibraryEnumStartKind::Standard,
            vec![1, 2, 3, 4, 5, 6],
        ),
        (StorageLibraryEnumStartKind::Chess960, vec![7]),
        (StorageLibraryEnumStartKind::Dfrc, vec![8]),
        (StorageLibraryEnumStartKind::Custom, vec![9, 10]),
    ] {
        fixture
            .expect_ids(
                StorageLibraryQueryOptions {
                    start_kind: StorageLibraryEnumFilter::Value(kind),
                    ..StorageLibraryQueryOptions::default()
                },
                &ids,
            )
            .await?;
    }
    for (variant, ids) in [
        (BoardEnumVariant::Standard, vec![9]),
        (BoardEnumVariant::Chess960, vec![10]),
    ] {
        fixture
            .expect_ids(
                StorageLibraryQueryOptions {
                    rules_variant: StorageLibraryEnumFilter::Value(variant),
                    start_kind: StorageLibraryEnumFilter::Value(
                        StorageLibraryEnumStartKind::Custom,
                    ),
                    ..StorageLibraryQueryOptions::default()
                },
                &ids,
            )
            .await?;
    }
    fixture
        .expect_ids(
            StorageLibraryQueryOptions {
                rules_variant: StorageLibraryEnumFilter::Value(BoardEnumVariant::Standard),
                start_kind: StorageLibraryEnumFilter::Value(StorageLibraryEnumStartKind::Dfrc),
                ..StorageLibraryQueryOptions::default()
            },
            &[],
        )
        .await?;
    fixture.close().await
}

#[tokio::test]
async fn opponent_mode_filters_require_actual_played_sessions() -> StorageLibraryFiltersTestResult {
    let fixture = StorageLibraryFiltersFixture::new().await?;
    fixture.representative_records().await?;
    for (mode, id, plies) in [
        (GameEnumMode::LocalTwoPlayer, 5, 2),
        (GameEnumMode::HumanVersusEngine, 6, 4),
    ] {
        let options = StorageLibraryQueryOptions {
            mode: StorageLibraryEnumFilter::Value(mode),
            ..StorageLibraryQueryOptions::default()
        };
        fixture.expect_ids(options.clone(), &[id]).await?;
        let page = fixture
            .repository()
            .query(StorageLibraryQuery::new(options)?)
            .await?;
        let entry = page.entries().first().ok_or("missing played query entry")?;
        assert_eq!(entry.mode(), Some(mode));
        assert_eq!(entry.plies(), plies);
        let loaded = fixture
            .repository()
            .load(entry.id())
            .await?
            .ok_or("missing played session")?;
        let StorageLibraryEnumContent::Played(session) = loaded.record().content() else {
            return Err("opponent-mode query returned notation".into());
        };
        assert_eq!(session.seats().mode(), mode);
        assert_eq!(session.live_moves()?.len(), usize::try_from(plies)?);
    }
    let page = fixture
        .repository()
        .query(StorageLibraryQuery::default())
        .await?;
    assert_eq!(
        page.entries()
            .iter()
            .filter(|entry| entry.mode().is_none())
            .count(),
        8
    );
    drop(page);
    fixture.close().await
}

#[tokio::test]
async fn favorites_and_folder_scope_select_exact_subtree_and_unfiled_membership()
-> StorageLibraryFiltersTestResult {
    let fixture = StorageLibraryFiltersFixture::new().await?;
    fixture.representative_records().await?;
    fixture.organization().await?;
    for (favorite, ids) in [(true, vec![1, 5]), (false, vec![2, 3, 4, 6, 7, 8, 9, 10])] {
        fixture
            .expect_ids(
                StorageLibraryQueryOptions {
                    favorite: StorageLibraryEnumFilter::Value(favorite),
                    ..StorageLibraryQueryOptions::default()
                },
                &ids,
            )
            .await?;
    }
    for (folder, ids) in [
        (
            StorageLibraryEnumFolderFilter::Exact(StorageLibraryFiltersFixture::folder(1_001)?),
            vec![1],
        ),
        (
            StorageLibraryEnumFolderFilter::Subtree(StorageLibraryFiltersFixture::folder(1_001)?),
            vec![1, 2],
        ),
        (
            StorageLibraryEnumFolderFilter::Exact(StorageLibraryFiltersFixture::folder(1_002)?),
            vec![2],
        ),
        (
            StorageLibraryEnumFolderFilter::Subtree(StorageLibraryFiltersFixture::folder(1_002)?),
            vec![2],
        ),
        (
            StorageLibraryEnumFolderFilter::Subtree(StorageLibraryFiltersFixture::folder(1_003)?),
            vec![5],
        ),
        (
            StorageLibraryEnumFolderFilter::Unfiled,
            vec![3, 4, 6, 7, 8, 9, 10],
        ),
        (
            StorageLibraryEnumFolderFilter::Exact(StorageLibraryFiltersFixture::folder(9_999)?),
            vec![],
        ),
    ] {
        fixture
            .expect_ids(
                StorageLibraryQueryOptions {
                    folder,
                    ..StorageLibraryQueryOptions::default()
                },
                &ids,
            )
            .await?;
    }
    fixture.close().await
}

#[tokio::test]
async fn tag_any_and_all_require_set_membership_and_exclude_untagged_games()
-> StorageLibraryFiltersTestResult {
    let fixture = StorageLibraryFiltersFixture::new().await?;
    fixture.representative_records().await?;
    fixture.organization().await?;
    for (tags, ids) in [
        (
            StorageLibraryEnumTagFilter::AnyOf(StorageLibraryFiltersFixture::tags(&[
                2_001, 2_002,
            ])?),
            vec![1, 2, 5],
        ),
        (
            StorageLibraryEnumTagFilter::AllOf(StorageLibraryFiltersFixture::tags(&[
                2_002, 2_001,
            ])?),
            vec![1],
        ),
        (
            StorageLibraryEnumTagFilter::AllOf(StorageLibraryFiltersFixture::tags(&[2_001])?),
            vec![1, 2],
        ),
        (
            StorageLibraryEnumTagFilter::AnyOf(StorageLibraryFiltersFixture::tags(&[2_003])?),
            vec![],
        ),
        (
            StorageLibraryEnumTagFilter::AllOf(StorageLibraryFiltersFixture::tags(&[
                2_001, 2_003,
            ])?),
            vec![],
        ),
    ] {
        fixture
            .expect_ids(
                StorageLibraryQueryOptions {
                    tags,
                    ..StorageLibraryQueryOptions::default()
                },
                &ids,
            )
            .await?;
    }
    for tags in [
        StorageLibraryEnumTagFilter::AnyOf(StorageLibraryFiltersFixture::tags(&[])?),
        StorageLibraryEnumTagFilter::AllOf(StorageLibraryFiltersFixture::tags(&[])?),
    ] {
        assert!(matches!(
            StorageLibraryQuery::new(StorageLibraryQueryOptions {
                tags,
                ..StorageLibraryQueryOptions::default()
            }),
            Err(StorageLibraryError::EmptyFilter)
        ));
    }
    fixture.close().await
}

#[tokio::test]
async fn opening_text_normalizes_unicode_but_keeps_question_mark_literal()
-> StorageLibraryFiltersTestResult {
    let fixture = StorageLibraryFiltersFixture::new().await?;
    fixture.representative_records().await?;
    for (text, ids) in [
        (" \u{2003} CAFE\u{301} ", vec![1, 2, 5]),
        ("gAMBIT", vec![1, 5]),
        ("?", vec![3]),
        ("Unknown opening", vec![]),
    ] {
        fixture
            .expect_ids(
                StorageLibraryQueryOptions {
                    opening_name: StorageLibraryEnumFilter::Value(
                        StorageLibraryFiltersFixture::text(text)?,
                    ),
                    ..StorageLibraryQueryOptions::default()
                },
                &ids,
            )
            .await?;
    }
    for (code, ids) in [("C20", vec![1, 5]), ("B01", vec![2]), ("E99", vec![])] {
        fixture
            .expect_ids(
                StorageLibraryQueryOptions {
                    opening_code: StorageLibraryEnumFilter::Value(GameMetadataOpeningCode::new(
                        code.to_owned(),
                    )?),
                    ..StorageLibraryQueryOptions::default()
                },
                &ids,
            )
            .await?;
    }
    fixture
        .expect_ids(
            StorageLibraryQueryOptions {
                opening_name: StorageLibraryEnumFilter::Value(StorageLibraryFiltersFixture::text(
                    "Café",
                )?),
                opening_code: StorageLibraryEnumFilter::Value(GameMetadataOpeningCode::new(
                    "B01".to_owned(),
                )?),
                ..StorageLibraryQueryOptions::default()
            },
            &[2],
        )
        .await?;
    for text in ["", " \u{2003} "] {
        assert!(matches!(
            StorageLibraryQuery::new(StorageLibraryQueryOptions {
                opening_name: StorageLibraryEnumFilter::Value(StorageLibraryFiltersFixture::text(
                    text
                )?),
                ..StorageLibraryQueryOptions::default()
            }),
            Err(StorageLibraryError::EmptyFilter)
        ));
    }
    fixture.close().await
}

#[tokio::test]
async fn ply_bounds_are_inclusive_for_notation_and_played_mainlines()
-> StorageLibraryFiltersTestResult {
    let fixture = StorageLibraryFiltersFixture::new().await?;
    fixture.representative_records().await?;
    for (minimum, maximum, ids) in [
        (0, 0, vec![3, 7, 8, 9, 10]),
        (1, 4, vec![1, 2, 4, 5, 6]),
        (2, 2, vec![1, 5]),
        (4, 4, vec![2, 6]),
        (5, 100_000, vec![]),
    ] {
        fixture
            .expect_ids(
                StorageLibraryQueryOptions {
                    plies: StorageLibraryEnumFilter::Value(StorageLibraryPlyInterval::new(
                        minimum, maximum,
                    )?),
                    ..StorageLibraryQueryOptions::default()
                },
                &ids,
            )
            .await?;
    }
    fixture.close().await
}

#[tokio::test]
async fn representative_filter_combinations_intersect_without_inventing_metadata()
-> StorageLibraryFiltersTestResult {
    let fixture = StorageLibraryFiltersFixture::new().await?;
    fixture.representative_records().await?;
    fixture.organization().await?;
    let notation = StorageLibraryQueryOptions {
        white: StorageLibraryEnumFilter::Value(StorageLibraryFiltersFixture::text("ÉLODIE")?),
        result: StorageLibraryEnumFilter::Value(NotationGameIdentityEnumResult::Completed(
            GameEnumResult::Victory(BoardEnumColor::White),
        )),
        rules_variant: StorageLibraryEnumFilter::Value(BoardEnumVariant::Standard),
        start_kind: StorageLibraryEnumFilter::Value(StorageLibraryEnumStartKind::Standard),
        favorite: StorageLibraryEnumFilter::Value(true),
        folder: StorageLibraryEnumFolderFilter::Subtree(StorageLibraryFiltersFixture::folder(
            1_001,
        )?),
        tags: StorageLibraryEnumTagFilter::AllOf(StorageLibraryFiltersFixture::tags(&[
            2_001, 2_002,
        ])?),
        opening_name: StorageLibraryEnumFilter::Value(StorageLibraryFiltersFixture::text(
            "cafe\u{301}",
        )?),
        opening_code: StorageLibraryEnumFilter::Value(GameMetadataOpeningCode::new(
            "C20".to_owned(),
        )?),
        plies: StorageLibraryEnumFilter::Value(StorageLibraryPlyInterval::new(2, 2)?),
        ..StorageLibraryQueryOptions::default()
    };
    fixture.expect_ids(notation.clone(), &[1]).await?;
    fixture
        .expect_ids(
            StorageLibraryQueryOptions {
                mode: StorageLibraryEnumFilter::Value(GameEnumMode::LocalTwoPlayer),
                ..notation
            },
            &[],
        )
        .await?;
    fixture
        .expect_ids(
            StorageLibraryQueryOptions {
                either_player: StorageLibraryEnumFilter::Value(StorageLibraryFiltersFixture::text(
                    "strasse",
                )?),
                mode: StorageLibraryEnumFilter::Value(GameEnumMode::LocalTwoPlayer),
                favorite: StorageLibraryEnumFilter::Value(true),
                folder: StorageLibraryEnumFolderFilter::Exact(
                    StorageLibraryFiltersFixture::folder(1_003)?,
                ),
                tags: StorageLibraryEnumTagFilter::AnyOf(StorageLibraryFiltersFixture::tags(&[
                    2_002,
                ])?),
                plies: StorageLibraryEnumFilter::Value(StorageLibraryPlyInterval::new(1, 2)?),
                ..StorageLibraryQueryOptions::default()
            },
            &[5],
        )
        .await?;
    fixture.close().await
}

#[tokio::test]
async fn partial_dates_use_exact_calendar_overlap_and_disclose_partial_matches()
-> StorageLibraryFiltersTestResult {
    let fixture = StorageLibraryFiltersFixture::new().await?;
    for (value, date) in [
        (1, Some(GameMetadataDate::new(Some(2024), None, Some(29))?)),
        (2, Some(GameMetadataDate::new(None, Some(2), Some(29))?)),
        (3, Some(GameMetadataDate::new(Some(2024), Some(2), None)?)),
        (
            4,
            Some(GameMetadataDate::new(Some(2024), Some(2), Some(29))?),
        ),
        (5, Some(GameMetadataDate::new(Some(2023), None, Some(29))?)),
        (6, Some(GameMetadataDate::new(None, None, Some(31))?)),
        (7, Some(GameMetadataDate::new(None, None, None)?)),
        (8, None),
        (9, Some(GameMetadataDate::new(Some(1900), None, Some(29))?)),
    ] {
        let id = StorageLibraryFiltersFixture::id(value)?;
        let body = StorageLibraryFiltersFixture::notation_body(
            id,
            StorageLibraryFiltersNotation {
                known: GameMetadataKnownTags {
                    date,
                    ..GameMetadataKnownTags::default()
                },
                ..StorageLibraryFiltersNotation::default()
            },
        )?;
        fixture.insert(id, body, 1_000).await?;
    }
    for (lower, upper, expected) in [
        (
            (2024, 2, 29),
            (2024, 2, 29),
            vec![(1, true), (2, true), (3, true), (4, false)],
        ),
        ((2024, 3, 1), (2024, 3, 28), vec![]),
        ((2023, 2, 28), (2023, 3, 1), vec![]),
        ((2024, 1, 30), (2024, 2, 1), vec![(3, true), (6, true)]),
        ((1900, 2, 28), (1900, 3, 1), vec![]),
        ((2000, 2, 29), (2000, 2, 29), vec![(2, true)]),
    ] {
        let options = StorageLibraryQueryOptions {
            date: StorageLibraryEnumFilter::Value(StorageLibraryDateInterval::new(
                GameMetadataDate::new(Some(lower.0), Some(lower.1), Some(lower.2))?,
                GameMetadataDate::new(Some(upper.0), Some(upper.1), Some(upper.2))?,
            )?),
            ..StorageLibraryQueryOptions::default()
        };
        let page = fixture
            .repository()
            .query(StorageLibraryQuery::new(options)?)
            .await?;
        assert!(page.next_cursor().is_none());
        let mut actual: Vec<_> = page
            .entries()
            .iter()
            .map(|entry| {
                (
                    entry.id().as_uuid().as_u128(),
                    entry.is_partial_date_match(),
                )
            })
            .collect();
        actual.sort_unstable();
        assert_eq!(actual, expected, "{lower:?}..={upper:?}");
    }
    let unfiltered = fixture
        .repository()
        .query(StorageLibraryQuery::default())
        .await?;
    assert_eq!(unfiltered.entries().len(), 9);
    assert!(
        unfiltered
            .entries()
            .iter()
            .all(|entry| !entry.is_partial_date_match())
    );
    drop(unfiltered);
    fixture.close().await
}

#[tokio::test]
async fn public_cursors_refuse_changed_filter_and_sort_criteria() -> StorageLibraryFiltersTestResult
{
    let fixture = StorageLibraryFiltersFixture::new().await?;
    fixture.cursor_records().await?;
    let page = fixture
        .repository()
        .query(StorageLibraryQuery::default())
        .await?;
    let cursor = page.next_cursor().cloned().ok_or("missing continuation")?;
    for options in [
        StorageLibraryQueryOptions {
            favorite: StorageLibraryEnumFilter::Value(true),
            ..StorageLibraryQueryOptions::default()
        },
        StorageLibraryQueryOptions {
            sort: StorageLibraryEnumSort::UpdatedAscending,
            ..StorageLibraryQueryOptions::default()
        },
        StorageLibraryQueryOptions {
            plies: StorageLibraryEnumFilter::Value(StorageLibraryPlyInterval::new(0, 0)?),
            ..StorageLibraryQueryOptions::default()
        },
    ] {
        assert!(matches!(
            StorageLibraryQuery::new(options)?.with_cursor(cursor.clone()),
            Err(StorageLibraryError::CursorQuery)
        ));
    }
    let next = fixture
        .repository()
        .query(StorageLibraryQuery::default().with_cursor(cursor)?)
        .await?;
    assert_eq!(next.entries().len(), 1);
    assert!(next.next_cursor().is_none());
    assert_eq!(
        next.entries()[0].id(),
        StorageLibraryFiltersFixture::id(101)?
    );
    drop(next);
    drop(page);
    fixture.close().await
}

#[tokio::test]
async fn reading_keeps_cursors_valid_and_checkpoint_keeps_modified_time()
-> StorageLibraryFiltersTestResult {
    let fixture = StorageLibraryFiltersFixture::new().await?;
    fixture.cursor_records().await?;
    let repository = fixture.repository();
    let page = repository.query(StorageLibraryQuery::default()).await?;
    let cursor = page.next_cursor().cloned().ok_or("missing continuation")?;
    let catalog = page.catalog_revision();
    drop(page);
    let id = StorageLibraryFiltersFixture::id(1)?;
    let loaded = repository
        .load(id)
        .await?
        .ok_or("missing record before viewing")?;
    let revision = loaded.content_revision();
    let modified = loaded.modified();
    assert_eq!(
        modified.map(GameLibraryOrganizationTimestamp::value),
        Some(1_000)
    );
    drop(loaded);
    let repeated = repository.query(StorageLibraryQuery::default()).await?;
    assert_eq!(repeated.catalog_revision(), catalog);
    drop(repeated);
    let continued = repository
        .query(StorageLibraryQuery::default().with_cursor(cursor.clone())?)
        .await?;
    assert_eq!(continued.catalog_revision(), catalog);
    assert_eq!(continued.entries().len(), 1);
    drop(continued);
    let body = StorageLibraryFiltersFixture::notation_body(
        id,
        StorageLibraryFiltersNotation {
            plies: 1,
            ..StorageLibraryFiltersNotation::default()
        },
    )?;
    let receipt = fixture
        .write(
            id,
            revision,
            body,
            StorageLibraryEnumContentChange::Checkpoint,
        )
        .await?;
    assert!(receipt.content_revision().value() > revision.value());
    assert!(receipt.catalog_revision().value() > catalog.value());
    assert!(matches!(
        repository
            .query(StorageLibraryQuery::default().with_cursor(cursor)?)
            .await,
        Err(StorageLibraryError::StaleCursor)
    ));
    let after = repository
        .load(id)
        .await?
        .ok_or("missing checkpointed record")?;
    assert_eq!(after.modified(), modified);
    assert_eq!(after.content_revision(), receipt.content_revision());
    drop(after);
    fixture.close().await
}

#[tokio::test]
async fn content_and_organization_mutations_refuse_existing_catalog_cursors()
-> StorageLibraryFiltersTestResult {
    let fixture = StorageLibraryFiltersFixture::new().await?;
    fixture.cursor_records().await?;
    let repository = fixture.repository();
    let before = repository.query(StorageLibraryQuery::default()).await?;
    let cursor = before
        .next_cursor()
        .cloned()
        .ok_or("missing continuation before organization")?;
    drop(before);
    let target = fixture.target(1).await?;
    let receipt = fixture
        .organize(GameLibraryOrganizationEnumMutation::SetFavorite(
            target, true,
        ))
        .await?;
    assert!(receipt.is_changed());
    assert!(matches!(
        repository
            .query(StorageLibraryQuery::default().with_cursor(cursor)?)
            .await,
        Err(StorageLibraryError::StaleCursor)
    ));
    let fresh = repository.query(StorageLibraryQuery::default()).await?;
    let cursor = fresh
        .next_cursor()
        .cloned()
        .ok_or("missing continuation before content edit")?;
    assert_eq!(
        fresh.entries().first().ok_or("missing newest game")?.id(),
        StorageLibraryFiltersFixture::id(1)?
    );
    drop(fresh);
    let id = StorageLibraryFiltersFixture::id(2)?;
    let loaded = repository.load(id).await?.ok_or("missing edited game")?;
    let revision = loaded.content_revision();
    drop(loaded);
    let body = StorageLibraryFiltersFixture::notation_body(
        id,
        StorageLibraryFiltersNotation {
            known: GameMetadataKnownTags {
                white: Some(StorageLibraryFiltersFixture::text("Edited player")?),
                ..GameMetadataKnownTags::default()
            },
            plies: 2,
            ..StorageLibraryFiltersNotation::default()
        },
    )?;
    fixture
        .write(
            id,
            revision,
            body,
            StorageLibraryEnumContentChange::Changed(GameLibraryOrganizationTimestamp::new(3_000)?),
        )
        .await?;
    assert!(matches!(
        repository
            .query(StorageLibraryQuery::default().with_cursor(cursor)?)
            .await,
        Err(StorageLibraryError::StaleCursor)
    ));
    let fresh = repository.query(StorageLibraryQuery::default()).await?;
    let newest = fresh
        .entries()
        .first()
        .ok_or("missing newest content edit")?;
    assert_eq!(newest.id(), id);
    assert_eq!(
        newest.white().map(|name| name.as_str()),
        Some("Edited player")
    );
    assert_eq!(
        newest
            .modified()
            .map(GameLibraryOrganizationTimestamp::value),
        Some(3_000)
    );
    drop(fresh);
    fixture.close().await
}

#[tokio::test]
async fn adopted_unknown_timestamp_survives_checkpoint_without_rewriting_on_adoption()
-> StorageLibraryFiltersTestResult {
    let fixture = StorageLibraryFiltersFixture::new().await?;
    let id = StorageLibraryFiltersFixture::id(1)?;
    let body = StorageLibraryFiltersFixture::parsed_body(
        id,
        "[White \"Legacy player\"]\n[Date \"2024.??.29\"]\n\n1. e4 e5 *",
    )?;
    let observed = fixture.adopt(id, body.clone()).await?;
    let receipt = fixture
        .write(
            id,
            observed,
            body,
            StorageLibraryEnumContentChange::Checkpoint,
        )
        .await?;
    let loaded = fixture
        .repository()
        .load(id)
        .await?
        .ok_or("missing adopted checkpoint")?;
    assert_eq!(loaded.modified(), None);
    assert_eq!(loaded.content_revision(), receipt.content_revision());
    assert!(loaded.content_revision().value() > observed.value());
    assert_eq!(
        loaded
            .record()
            .metadata()
            .known()
            .white
            .as_ref()
            .map(|text| text.as_str()),
        Some("Legacy player")
    );
    assert_eq!(loaded.record().identity_line_len()?, 2);
    drop(loaded);
    fixture.close().await
}
