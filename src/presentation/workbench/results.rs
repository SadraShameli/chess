use crate::package::digest::PackageDigest;
use crate::workbench::export::{
    WorkbenchExport, WorkbenchExportContext, WorkbenchExportEncoded, WorkbenchExportError,
    WorkbenchExportMetadata, WorkbenchExportPort, WorkbenchExportReceipt,
    WorkbenchExportWriteError,
};
use crate::workbench::job::WorkbenchJob;

pub struct PresentationWorkbenchResults {
    encoded: WorkbenchExportEncoded,
    diagnostic_lines: usize,
    evicted_lines: u64,
    evicted_bytes: u64,
}

impl PresentationWorkbenchResults {
    pub fn preview(
        job: &WorkbenchJob,
        context: &WorkbenchExportContext,
        metadata: &WorkbenchExportMetadata,
    ) -> Result<Self, WorkbenchExportError> {
        let export = WorkbenchExport::capture(job, context)?;
        Ok(Self {
            encoded: export.prepare(metadata)?,
            diagnostic_lines: export.diagnostic_lines(),
            evicted_lines: export.evicted_lines(),
            evicted_bytes: export.evicted_bytes(),
        })
    }

    pub const fn diagnostic_lines(&self) -> usize {
        self.diagnostic_lines
    }

    pub const fn evicted_lines(&self) -> u64 {
        self.evicted_lines
    }

    pub const fn evicted_bytes(&self) -> u64 {
        self.evicted_bytes
    }

    pub const fn has_omitted_diagnostics(&self) -> bool {
        self.evicted_lines > 0 || self.evicted_bytes > 0
    }

    pub fn bytes(&self) -> &[u8] {
        self.encoded.bytes()
    }

    pub const fn length(&self) -> u64 {
        self.encoded.length()
    }

    pub const fn sha256(&self) -> PackageDigest {
        self.encoded.sha256()
    }

    pub fn write<P: WorkbenchExportPort>(
        &self,
        port: &mut P,
    ) -> Result<WorkbenchExportReceipt, WorkbenchExportWriteError<P::Error>> {
        self.encoded.write(port)
    }
}
