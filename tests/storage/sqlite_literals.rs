pub(crate) struct StorageSqliteLiterals;

impl StorageSqliteLiterals {
    pub(crate) fn text(text: &str) -> String {
        text.split('\0')
            .map(|part| format!("'{}'", part.replace('\'', "''")))
            .collect::<Vec<_>>()
            .join(" || char(0) || ")
    }

    pub(crate) fn blob(bytes: &[u8]) -> String {
        let digits: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        format!("X'{digits}'")
    }

    pub(crate) fn decode_hex(hex: &str) -> Result<Vec<u8>, std::num::ParseIntError> {
        (0..hex.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&hex[index..index + 2], 16))
            .collect()
    }
}
