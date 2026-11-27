use std::sync::Arc;

use crate::engine::MeterEngine;
use crate::shared::SharedMeters;
use crate::ShirabeParams;

#[test]
fn cadence_and_vu_follow_audio_time_under_the_allocation_guard() {
    for rate in [44_100.0, 48_000.0, 96_000.0] {
        let interval = (rate / 200.0_f32).round() as usize;
        let input: Vec<_> = (0..interval * 40 + 23)
            .map(|i| (i as f32 * 0.125).sin() * (i as f32 / rate))
            .collect();
        let mut expected = Vec::new();
        for block in [1, 127, 512, 2048, 8192] {
            let shared = Arc::new(SharedMeters::new());
            let mut engine = MeterEngine::new(Arc::new(ShirabeParams::default()), shared.clone());
            engine.prepare(rate, 1, 0);
            let mut audio = input.clone();
            for block in audio.chunks_mut(block) {
                assert_no_alloc::assert_no_alloc(|| engine.process(&mut [block], &mut [], false));
            }
            let points = shared.timeline.points_for_test();
            assert_eq!(points.len(), 40);
            for (i, point) in points.iter().enumerate() {
                assert_eq!(point.frame, ((i + 1) * interval) as u64);
            }
            let vu: Vec<_> = points.iter().map(|point| point.channels[0].vu).collect();
            if block == 1 {
                expected = vu;
            } else {
                assert_eq!(vu, expected);
            }
        }
    }
}

#[test]
fn timed_snapshots_preserve_host_block_measurements_bit_for_bit() {
    use std::sync::atomic::Ordering::Relaxed;

    use nih_plug::prelude::Param;
    use shirabe_dsp::{
        ChannelLayout, LoudnessMeter, PeakMeter, PpmMeter, PpmType, TruePeakMeter, VuMeter,
    };

    use crate::params::LevelMode;
    const RATE: f32 = 48_000.0;
    for (mode, kind) in [
        (LevelMode::Vu, PpmType::TypeII),
        (LevelMode::PpmTypeI, PpmType::TypeI),
        (LevelMode::PpmTypeII, PpmType::TypeII),
    ] {
        let params = Arc::new(ShirabeParams::default());
        unsafe {
            params
                .level_mode
                .as_ptr()
                .set_normalized_value(params.level_mode.preview_normalized(mode));
        }
        assert_eq!(params.level_mode.value(), mode);
        let shared = Arc::new(SharedMeters::new());
        let mut engine = MeterEngine::new(params.clone(), shared.clone());
        engine.prepare(RATE, 1, 0);
        let mut vu = VuMeter::new(RATE, params.vu_reference.value());
        let mut ppm = PpmMeter::new(RATE, kind);
        let mut peak = PeakMeter::new(RATE, Default::default());
        let mut tp = TruePeakMeter::new(RATE);
        let mut loudness = LoudnessMeter::new(RATE, ChannelLayout::Mono.roles());
        let mut offset = 0;
        for (index, block) in [8192, 127, 2048, 1]
            .into_iter()
            .cycle()
            .take(60)
            .enumerate()
        {
            // Quiet, loud and silent blocks exercise the detector tails, peaks and loudness gates.
            let gain = match index % 5 {
                0 => 0.0,
                1 => 1e-15,
                _ => 0.3 - index as f32 * 0.003,
            };
            let mut input: Vec<_> = (offset..offset + block)
                .map(|i| ((i % 48) as f32 * std::f32::consts::TAU / 48.0).sin() * gain)
                .collect();
            offset += block;
            vu.process(&input);
            ppm.process(&input);
            peak.process(&input);
            tp.process(&input);
            loudness.process(&[&input]);
            assert_no_alloc::assert_no_alloc(|| engine.process(&mut [&mut input], &mut [], false));
            let channel = shared.channel(0);
            match mode {
                LevelMode::Vu => assert_eq!(channel.vu.load(Relaxed).to_bits(), vu.vu().to_bits()),
                _ => assert_eq!(
                    channel.ppm_db.load(Relaxed).to_bits(),
                    ppm.level_db().to_bits()
                ),
            }
            assert_eq!(
                channel.level_db.load(Relaxed).to_bits(),
                peak.level_db().to_bits()
            );
            assert_eq!(
                channel.hold_db.load(Relaxed).to_bits(),
                peak.hold_db().to_bits()
            );
            assert_eq!(
                channel.max_db.load(Relaxed).to_bits(),
                peak.max_db().to_bits()
            );
            assert_eq!(
                channel.true_peak_max_db.load(Relaxed).to_bits(),
                tp.max_dbtp().to_bits()
            );
            let measured = loudness.readout();
            for (expected, actual) in [
                (measured.momentary, &shared.loudness.momentary),
                (measured.short_term, &shared.loudness.short_term),
                (measured.integrated, &shared.loudness.integrated),
                (measured.range, &shared.loudness.range),
                (measured.max_momentary, &shared.loudness.max_momentary),
                (measured.max_short_term, &shared.loudness.max_short_term),
            ] {
                assert_eq!(
                    actual.load().map(f32::to_bits),
                    expected.map(|v| (v as f32).to_bits())
                );
            }
        }
    }
}

fn ramp_point(frame: u64) -> crate::timeline::Snapshot {
    let mut point = crate::timeline::Snapshot {
        frame,
        ..Default::default()
    };
    point.channels[0].vu = -20.0 + 3.0 * frame as f32 / 48_000.0;
    point
}

#[test]
fn burst_playout_advances_smoothly_at_60_hz() {
    use crate::playout::Playout;

    const RATE: u64 = 48_000;
    const BLOCK: u64 = 8_192;
    const TICK: u64 = RATE / 60;
    const INTERVAL: u64 = RATE / 200;
    let shared = SharedMeters::new();
    shared.configure(2, RATE as f32, false);
    let mut playout = Playout::default();
    let mut delivered = 0;
    let mut next_point = INTERVAL;
    let mut previous = None;
    let (mut raw_jump, mut played_jump, mut min_lag, mut max_lag) =
        (0.0f32, 0.0f32, 1.0f64, 0.0f64);
    for audio_now in (0..RATE * 6).step_by(TICK as usize) {
        while delivered <= audio_now {
            delivered += BLOCK;
            while next_point <= delivered {
                shared.timeline.push(ramp_point(next_point));
                next_point += INTERVAL;
            }
        }
        let point = playout
            .update(&shared, audio_now as f64 / RATE as f64)
            .unwrap();
        let raw = ramp_point(delivered).channels[0].vu;
        let played = point.channels[0].vu;
        if let Some((prev_played, prev_raw)) = previous {
            let jump = played - prev_played;
            assert!(jump >= 0.0, "the ramp went backwards: {jump}");
            if audio_now > RATE / 2 {
                assert!(jump > 0.0, "the ramp stalled at {audio_now}");
                let lag = (audio_now + BLOCK - point.frame) as f64 / RATE as f64;
                min_lag = min_lag.min(lag);
                max_lag = max_lag.max(lag);
            }
            played_jump = played_jump.max(jump);
            raw_jump = raw_jump.max(raw - prev_raw);
        }
        previous = Some((played, raw));
    }
    println!(
        "steady ramp (3 VU/s): legacy {raw_jump:.4}, playout {played_jump:.4} VU/frame; lag {:.1}–{:.1} ms",
        min_lag * 1000.0,
        max_lag * 1000.0,
    );
    assert!(played_jump < 0.075 && raw_jump > 0.5);
    assert!(min_lag >= 0.01 && max_lag <= 0.25);
}

#[test]
fn playout_drops_the_old_epoch_and_holds_when_starved() {
    use crate::playout::Playout;
    let mut playout = Playout::default();
    let shared = SharedMeters::new();
    shared.configure(2, 48_000.0, false);
    shared.timeline.push(ramp_point(480));
    shared.timeline.push(ramp_point(720));
    assert_eq!(playout.update(&shared, 0.0).unwrap().frame, 480);
    assert_eq!(playout.update(&shared, 1.0).unwrap().frame, 720);
    assert_eq!(playout.update(&shared, 2.0).unwrap().frame, 720);
    shared.configure(2, 96_000.0, false);
    assert!(playout.update(&shared, 2.01).is_none());
    shared.timeline.push(ramp_point(240));
    assert_eq!(playout.update(&shared, 2.02).unwrap().frame, 240);
}

#[test]
fn timeline_wrap_keeps_only_complete_newest_points() {
    use crate::timeline::{Timeline, CAPACITY};
    let timeline = Timeline::new();
    for i in 0..CAPACITY + 23 {
        timeline.push(ramp_point((i + 1) as u64));
    }
    let points = timeline.points_for_test();
    assert_eq!(points.len(), CAPACITY);
    assert_eq!(
        points,
        (24..=CAPACITY as u64 + 23)
            .map(ramp_point)
            .collect::<Vec<_>>()
    );
    timeline.clear();
    assert!(timeline.points_for_test().is_empty());
    timeline.push(ramp_point(240));
    assert_eq!(timeline.points_for_test(), [ramp_point(240)]);
}
