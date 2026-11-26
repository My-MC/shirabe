use std::sync::{atomic::Ordering, Arc};

use shirabe_plugin::{engine::MeterEngine, shared::SharedMeters, ShirabeParams};

fn feed(engine: &mut MeterEngine, peak_db: f32) {
    let gain = 10.0_f32.powf(peak_db / 20.0);
    let mut signal: Vec<_> = (0..48_000)
        .map(|i| gain * (std::f32::consts::TAU * (i % 48) as f32 / 48.0).sin())
        .collect();
    engine.process(&mut [&mut signal], false);
}

#[test]
fn plr_uses_the_programme_peak_and_waits_for_integrated_loudness() {
    let shared = Arc::new(SharedMeters::new());
    let mut engine = MeterEngine::new(Arc::new(ShirabeParams::default()), shared.clone());
    engine.prepare(48_000.0, 1);
    assert_eq!(shared.loudness.plr.load(), None);

    feed(&mut engine, -18.0);
    let plr = shared.loudness.plr.load().unwrap();
    assert!((plr - 3.0).abs() < 0.1, "{plr}");

    shared.set_integrating(false);
    feed(&mut engine, -3.0);
    assert_eq!(shared.loudness.plr.load(), Some(plr));
    assert!(shared.channel(0).true_peak_max_db.load(Ordering::Relaxed) > -4.0);

    shared.set_integrating(true);
    feed(&mut engine, -18.0);
    assert!((shared.loudness.plr.load().unwrap() - plr).abs() < 0.1);

    shared.request_reset();
    let mut silence = [0.0; 480];
    engine.process(&mut [&mut silence], false);
    assert_eq!(shared.loudness.plr.load(), None);
    feed(&mut engine, -24.0);
    assert!((shared.loudness.plr.load().unwrap() - plr).abs() < 0.1);
    engine.reset();
    assert_eq!(shared.loudness.plr.load(), None);
}
