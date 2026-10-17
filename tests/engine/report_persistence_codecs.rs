use strum::IntoEnumIterator;
#[path = "report_persistence_codecs/analysis.rs"]
mod analysis;
#[path = "report_persistence_codecs/fixture.rs"]
mod fixture;
#[path = "report_persistence_codecs/metadata.rs"]
mod metadata;
#[path = "report_persistence_codecs/runtime.rs"]
mod runtime;

type EngineReportPersistenceTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct EngineReportPersistenceFixture;

impl EngineReportPersistenceFixture {
    fn roundtrip<T>(value: &T) -> EngineReportPersistenceTestResult<T>
    where
        T: serde::Serialize + serde::de::DeserializeOwned,
    {
        let json = serde_json::to_string(value)?;
        Ok(serde_json::from_str(&json)?)
    }

    fn capabilities(
        maximum: u32,
    ) -> EngineReportPersistenceTestResult<chess::engine::capabilities::EngineCapabilities> {
        let mut capabilities = chess::engine::capabilities::EngineCapabilities::default();
        for option in chess::engine::enums::EngineEnumOption::iter() {
            let descriptor = if option == chess::engine::enums::EngineEnumOption::Threads {
                format!("spin default 1 min 1 max {maximum}")
            } else {
                option.descriptor()
            };
            capabilities.discover(&format!("option name {} type {descriptor}", option.name()))?;
        }
        capabilities.validate()?;
        Ok(capabilities)
    }
}
