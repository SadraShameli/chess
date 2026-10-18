use std::path::PathBuf;

use crate::engine::pgo::{
    ToolsEnginePgoEnumBuild, ToolsEnginePgoEnumDisposition, ToolsEnginePgoError,
    ToolsEnginePgoInputs, ToolsEnginePgoJournal, ToolsEnginePgoResources,
};

pub(crate) struct ToolsEngineSpsaCheck;

impl ToolsEngineSpsaCheck {
    pub(crate) async fn run(
        inputs: ToolsEnginePgoInputs,
        resources: ToolsEnginePgoResources<'_>,
    ) -> Result<PathBuf, ToolsEnginePgoError> {
        let mut journal = ToolsEnginePgoJournal::new(&inputs, resources).await?;
        let result = async {
            journal.verify(&inputs).await?;
            let (_binary, baseline) = journal.build(ToolsEnginePgoEnumBuild::Baseline).await?;
            baseline.require_success()?;
            let (_binary, spsa) = journal.build(ToolsEnginePgoEnumBuild::Spsa).await?;
            journal.verify_unchanged().await?;
            match spsa.require_success() {
                Ok(()) => Ok(ToolsEnginePgoEnumDisposition::SpsaCompileSucceeded),
                Err(ToolsEnginePgoError::Command { .. })
                    if Self::is_upstream_unavailable(spsa.stderr()) =>
                {
                    Ok(ToolsEnginePgoEnumDisposition::SpsaUpstreamUnavailable)
                }
                Err(error) => Err(error),
            }
        }
        .await;
        journal.finish(result).await
    }

    pub(crate) fn is_upstream_unavailable(stderr: &str) -> bool {
        const FUNCTIONS: [&str; 2] = ["print_options", "set_parameter"];
        let diagnostic_count = stderr
            .lines()
            .filter(|line| line.starts_with("error["))
            .count();
        diagnostic_count == FUNCTIONS.len()
            && !stderr.lines().any(|line| {
                line.starts_with("error:")
                    && !line.starts_with("error: could not compile `reckless`")
                    && !line.starts_with("error: aborting due to 2 previous errors")
            })
            && FUNCTIONS.into_iter().all(|function| {
                stderr.lines().any(|line| {
                    line.contains("error[E0425]")
                        && line.contains(&format!(
                            "cannot find function `{function}` in module `crate::parameters`"
                        ))
                })
            })
    }
}
