use crate::clock::duration::ClockInstant;
use crate::presentation::timing::PresentationTimingSampler;

fn at(millis: u64) -> ClockInstant {
    ClockInstant::from_nanos(millis * 1_000_000)
}

#[test]
fn inputs_pair_with_the_next_presented_frame_and_active_frames_record_intervals()
-> Result<(), Box<dyn std::error::Error>> {
    let mut sampler = PresentationTimingSampler::new();
    sampler.record_input(at(0));
    sampler.record_input(at(5));
    sampler.record_presented(at(16), true)?;
    sampler.record_presented(at(33), true)?;
    sampler.record_presented(at(50), false)?;
    assert_eq!(sampler.input_samples(), 2);
    assert_eq!(sampler.frame_samples(), 1);
    assert_eq!(sampler.input_p95().map(|value| value.as_millis()), Some(16));
    assert_eq!(sampler.frame_p95().map(|value| value.as_millis()), Some(17));
    Ok(())
}

#[test]
fn nearest_rank_uses_the_ceiling_rank_and_json_lists_every_sample()
-> Result<(), Box<dyn std::error::Error>> {
    let mut sampler = PresentationTimingSampler::new();
    for index in 0..20_u64 {
        sampler.record_input(at(index * 100));
        sampler.record_presented(at(index * 100 + index + 1), true)?;
    }
    assert_eq!(sampler.input_p95().map(|value| value.as_millis()), Some(19));
    let json = sampler.to_json();
    assert!(json.starts_with("{\"input_ms\":[1.000000,2.000000"));
    assert!(json.contains("\"frame_ms\":["));
    Ok(())
}

#[test]
fn a_frame_before_its_predecessor_is_refused() -> Result<(), Box<dyn std::error::Error>> {
    let mut sampler = PresentationTimingSampler::new();
    sampler.record_presented(at(10), true)?;
    assert!(sampler.record_presented(at(5), true).is_err());
    Ok(())
}
