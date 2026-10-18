use crate::engine::search::enums::EngineSearchEnumOutput;
use crate::engine::search::identity::EngineSearchTag;

#[derive(Debug)]
pub struct EngineSearchEvent {
    tag: EngineSearchTag,
    output: EngineSearchEnumOutput,
}

impl EngineSearchEvent {
    pub(crate) fn new(tag: EngineSearchTag, output: EngineSearchEnumOutput) -> Self {
        Self { tag, output }
    }

    pub const fn tag(&self) -> EngineSearchTag {
        self.tag
    }

    pub fn output(&self) -> &EngineSearchEnumOutput {
        &self.output
    }

    pub fn into_output(self) -> EngineSearchEnumOutput {
        self.output
    }
}
