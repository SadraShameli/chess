use chess::engine::capabilities::{EngineCapabilities, EngineCapabilitiesError};
use chess::engine::enums::{EngineEnumOption, EngineOptionEnumDescriptor};

#[test]
fn dynamic_threads_descriptor_accepts_the_advertised_high_core_bound()
-> Result<(), Box<dyn std::error::Error>> {
    for maximum in [516, 1024] {
        let mut capabilities = EngineCapabilities::default();
        let line = format!("option name Threads type spin default 1 min 1 max {maximum}");
        capabilities.discover(&line)?;
        assert_eq!(
            capabilities.options().collect::<Vec<_>>(),
            [EngineEnumOption::Threads]
        );
        let Some(EngineOptionEnumDescriptor::Spin(range)) =
            capabilities.descriptor(EngineEnumOption::Threads)
        else {
            return Err("missing typed Threads descriptor".into());
        };
        assert_eq!(range.default_value(), 1);
        assert_eq!(range.minimum(), 1);
        assert_eq!(range.maximum(), maximum);
        assert!(range.contains(u64::from(maximum)));
        assert!(!range.contains(u64::from(maximum) + 1));
        assert!(
            capabilities
                .discover("option name Threads type spin default 1 min 1 max 2048")
                .is_err()
        );
        assert_eq!(
            capabilities.descriptor(EngineEnumOption::Threads),
            Some(EngineOptionEnumDescriptor::Spin(range))
        );
    }
    Ok(())
}

#[test]
fn fixed_descriptors_remain_exact_and_every_duplicate_is_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    let mut capabilities = EngineCapabilities::default();
    for option in EngineEnumOption::ALL {
        let line = format!("option name {} type {}", option.name(), option.descriptor());
        capabilities.discover(&line)?;
        assert_eq!(
            capabilities.descriptor(option),
            Some(option.typed_descriptor())
        );
        assert!(capabilities.discover(&line).is_err());
        assert!(
            EngineCapabilities::default()
                .discover(&format!("{line} extra"))
                .is_err()
        );
    }
    capabilities.validate()?;
    for line in [
        "option name Threads type spin default 2 min 1 max 1024",
        "option name Threads type spin default 1 min 0 max 1024",
        "option name Threads type spin default 1 min 2 max 1024",
        "option name Threads type spin default 1 min 1 max 511",
        "option name Threads type spin default 1 min 1 max 513",
        "option name Threads type spin default 1 min 1 max 0",
        "option name Threads type spin default 1 min 1 max -1024",
        "option name Threads type spin default 1 min 1 max +1024",
        "option name Threads type spin default 1 min 1 max 01024",
        "option name Threads type spin default 1 min 1 max 4294967296",
        "option name Threads type spin default 1 min 1 max broken",
        "option name Threads type check default false",
        "option name Threads",
        "option name Hash type spin default 32 min 1 max 262144",
        "option name Hash type spin default 16 min 0 max 262144",
        "option name Hash type spin default 16 min 1 max 262145",
        "option name MoveOverhead type spin default 0 min 0 max 2000",
        "option name MoveOverhead type spin default 100 min 1 max 2000",
        "option name MoveOverhead type spin default 100 min 0 max 2001",
        "option name MultiPV type spin default 1 min 1 max 1024",
        "option name Minimal type check default true",
        "option name UCI_Chess960 type check default true",
        "option name Clear Hash type button default false",
        "option name SyzygyPath type string default /tables",
        "option name FutureOption type spin default 1 min 1 max 1024",
    ] {
        assert!(
            EngineCapabilities::default().discover(line).is_err(),
            "{line}"
        );
    }
    Ok(())
}

#[test]
fn unknown_advertisement_retains_its_name_and_descriptor_as_a_typed_coverage_failure() {
    let mut capabilities = EngineCapabilities::default();
    assert!(matches!(
        capabilities.discover("option name Future Option type spin default 2 min 1 max 1024"),
        Err(EngineCapabilitiesError::UnknownOption { name, descriptor })
            if name == "Future Option" && descriptor == "spin default 2 min 1 max 1024"
    ));
    assert_eq!(capabilities.options().len(), 0);
    assert!(matches!(
        capabilities.validate(),
        Err(EngineCapabilitiesError::MissingOptions)
    ));
}
