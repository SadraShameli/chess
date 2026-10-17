use std::collections::BTreeSet;

use chess::engine::enums::EngineEnumOption;
use chess::workbench::enums::WorkbenchEnumOperation;

struct WorkbenchContractTable;

impl WorkbenchContractTable {
    const SOURCE: &str =
        include_str!("../../specs/001-reckless-desktop-chess/contracts/reckless-capabilities.md");

    fn rows() -> Vec<(String, String)> {
        Self::SOURCE
            .lines()
            .filter_map(|line| {
                let mut cells = line.split('|').map(str::trim);
                cells.next()?;
                let id = cells.next()?;
                let facility = cells.next()?;
                let (prefix, ordinal) = id.split_once('-')?;
                let valid = !prefix.is_empty()
                    && prefix
                        .chars()
                        .all(|character| character.is_ascii_uppercase())
                    && ordinal.len() == 2
                    && ordinal.chars().all(|character| character.is_ascii_digit());
                valid.then(|| (id.to_owned(), facility.trim_matches('`').to_owned()))
            })
            .collect()
    }
}

#[test]
fn contract_rows_are_unique_and_option_rows_match_the_option_registry() {
    let rows = WorkbenchContractTable::rows();
    let ids: BTreeSet<&str> = rows.iter().map(|(id, _facility)| id.as_str()).collect();
    assert_eq!(ids.len(), rows.len());
    let options: Vec<String> = rows
        .iter()
        .filter(|(id, _facility)| id.starts_with("OPT-"))
        .map(|(_id, facility)| facility.clone())
        .collect();
    let registry: Vec<String> = EngineEnumOption::ALL
        .iter()
        .map(|option| option.name().to_owned())
        .collect();
    assert_eq!(options, registry);
}

#[test]
fn developer_rows_name_each_workbench_operation_once_then_the_non_workbench_facilities() {
    let facilities: Vec<String> = WorkbenchContractTable::rows()
        .into_iter()
        .filter(|(id, _facility)| id.starts_with("DEV-"))
        .map(|(_id, facility)| facility)
        .collect();
    assert_eq!(facilities.len(), 8);
    let commands: Vec<&str> = WorkbenchEnumOperation::ALL
        .iter()
        .map(|operation| match operation {
            WorkbenchEnumOperation::Inspector => "d",
            WorkbenchEnumOperation::RawEvaluation => "eval",
            WorkbenchEnumOperation::Compiler => "compiler",
            WorkbenchEnumOperation::Perft => "perft depth",
            WorkbenchEnumOperation::Bench => "bench [hash [threads [depth]]]",
        })
        .collect();
    assert_eq!(&facilities[..commands.len()], commands.as_slice());
    assert_eq!(
        &facilities[commands.len()..],
        [
            "CLI queued-command mode",
            "PGO release workflow",
            "Optional `spsa` feature"
        ]
    );
}
