use std::ffi::OsStr;
use std::io::Write;

use chess::desktop::schema::DesktopSchema;
use thiserror::Error;

#[derive(Debug, Error)]
enum ToolsCommandError {
    #[error("Usage: chess-tools schema | schema-fixture")]
    Usage,
}

enum ToolsEnumCommand {
    Schema,
    SchemaFixture,
}

impl ToolsEnumCommand {
    fn parse() -> Result<Self, ToolsCommandError> {
        let mut args = std::env::args_os().skip(1);
        match (args.next().as_deref(), args.next()) {
            (Some(command), None) if command == OsStr::new("schema") => Ok(Self::Schema),
            (Some(command), None) if command == OsStr::new("schema-fixture") => {
                Ok(Self::SchemaFixture)
            }
            _ => Err(ToolsCommandError::Usage),
        }
    }

    fn execute(self) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Self::Schema => {
                let mut output = std::io::stdout().lock();
                serde_json::to_writer_pretty(&mut output, &DesktopSchema::generate())?;
                writeln!(output)?;
            }
            Self::SchemaFixture => {
                let mut output = std::io::stdout().lock();
                serde_json::to_writer_pretty(&mut output, &DesktopSchema::fixture()?)?;
                writeln!(output)?;
            }
        }
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    ToolsEnumCommand::parse()?.execute()
}
