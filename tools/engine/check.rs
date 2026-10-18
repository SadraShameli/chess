use std::io::Write;
use std::path::PathBuf;

use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::CapacityExecutor;
use chess::engine::package::host::EnginePackageHost;
use chess::engine::package::selector::EnginePackageSelector;
use chess::engine::process_provider::EngineNativeProcessProvider;
use chess::engine::session::EngineSession;
use serde::Serialize;

#[derive(Serialize)]
struct ToolsEngineCheckReceipt<'a> {
    name: &'a str,
    author: &'a str,
    target: &'a str,
    binary_sha256: String,
    options: Vec<&'a str>,
}

pub(crate) struct ToolsEngineCheck;

impl ToolsEngineCheck {
    pub(crate) async fn run() -> Result<(), Box<dyn std::error::Error>> {
        let executor = CapacityExecutor::new(CapacityBudget::discover()?)?;
        let selected = executor
            .submit(|_cancellation| {
                let host = EnginePackageHost::detect()?;
                let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/engine");
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(EnginePackageSelector::load(
                    &root, host,
                )?)
            })?
            .join()
            .await?;
        executor.shutdown().await?;
        let selected = selected.map_err(|error| -> Box<dyn std::error::Error> { error })?;
        let session = EngineSession::start(&EngineNativeProcessProvider, selected).await?;
        let identity = session.identity();
        let receipt = ToolsEngineCheckReceipt {
            name: identity.name(),
            author: identity.author(),
            target: session.package().target().triple(),
            binary_sha256: session.package().digest().to_string(),
            options: identity
                .capabilities()
                .options()
                .map(|option| option.name())
                .collect(),
        };
        let output = Self::write(&receipt);
        session.shutdown().await?;
        output?;
        Ok(())
    }

    fn write(receipt: &ToolsEngineCheckReceipt<'_>) -> Result<(), Box<dyn std::error::Error>> {
        let mut output = std::io::stdout().lock();
        serde_json::to_writer_pretty(&mut output, receipt)?;
        writeln!(output)?;
        Ok(())
    }
}
