//! Sidechain reference comparison.
//!
//! The plug-in can take a second input (the host's sidechain bus) and measure it with an
//! independent loudness meter, so the main measurement can be compared against a reference
//! programme. The sidechain must never reach the output, the comparison must follow the same
//! reset and pause semantics as the main measurement, and an absent reference must read as
//! unavailable instead of zero.

use std::f32::consts::TAU;
use std::sync::Arc;

use nih_plug::prelude::Plugin;
use shirabe_plugin::engine::MeterEngine;
use shirabe_plugin::shared::SharedMeters;
use shirabe_plugin::{Shirabe, ShirabeParams};

const RATE: f32 = 48_000.0;
const BLOCK: usize = 512;

/// One running engine with the parameters and read-outs the editor would see.
struct Rig {
    engine: MeterEngine,
    shared: Arc<SharedMeters>,
}

fn rig(channels: usize, reference_channels: usize) -> Rig {
    let params = Arc::new(ShirabeParams::default());
    let shared = Arc::new(SharedMeters::new());
    let mut engine = MeterEngine::new(params, shared.clone());
    engine.prepare(RATE, channels, reference_channels);
    Rig { engine, shared }
}

/// A continuous sine generator, so block boundaries do not click.
struct Tone {
    phase: f32,
    increment: f32,
}

impl Tone {
    fn new(frequency: f32) -> Self {
        Self {
            phase: 0.0,
            increment: TAU * frequency / RATE,
        }
    }

    fn fill(&mut self, buffer: &mut [f32], amplitude: f32) {
        for sample in buffer.iter_mut() {
            *sample = amplitude * self.phase.sin();
            self.phase = (self.phase + self.increment) % TAU;
        }
    }
}

/// Runs `frames` frames of a main tone and a reference tone (amplitude 0 = silence) through
/// the engine, in [`BLOCK`]-sized blocks.
fn run(
    rig: &mut Rig,
    frames: usize,
    main: [f32; 2],
    reference: [f32; 2],
    reference_channels: usize,
) {
    let mut main_tone = Tone::new(1000.0);
    let mut reference_tone = Tone::new(1000.0);
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    let mut ref_left = vec![0.0f32; BLOCK];
    let mut ref_right = vec![0.0f32; BLOCK];
    let mut done = 0;
    while done < frames {
        let count = BLOCK.min(frames - done);
        main_tone.fill(&mut left[..count], main[0]);
        main_tone.fill(&mut right[..count], main[1]);
        reference_tone.fill(&mut ref_left[..count], reference[0]);
        reference_tone.fill(&mut ref_right[..count], reference[1]);
        let mut channels: Vec<&mut [f32]> = vec![&mut left[..count], &mut right[..count]];
        let mut sidechain: Vec<&mut [f32]> = match reference_channels {
            0 => Vec::new(),
            1 => vec![&mut ref_left[..count]],
            _ => vec![&mut ref_left[..count], &mut ref_right[..count]],
        };
        rig.engine.process(&mut channels, &mut sidechain, false);
        done += count;
    }
}

#[test]
fn the_sidechain_never_reaches_the_output() {
    // Two engines on the same main signal; only one of them has a loud sidechain. The main
    // output must be bit-identical, i.e. the sidechain is measured but never mixed in.
    let mut with_reference = rig(2, 2);
    let mut without_reference = rig(2, 0);
    let mut main_tone_a = Tone::new(1000.0);
    let mut main_tone_b = Tone::new(1000.0);
    let mut noise_phase = 0.0f32;

    for block in 0..60 {
        let mut left_a = vec![0.0f32; BLOCK];
        let mut right_a = vec![0.0f32; BLOCK];
        let mut left_b = vec![0.0f32; BLOCK];
        let mut right_b = vec![0.0f32; BLOCK];
        main_tone_a.fill(&mut left_a, 0.2);
        main_tone_a.fill(&mut right_a, 0.2);
        main_tone_b.fill(&mut left_b, 0.2);
        main_tone_b.fill(&mut right_b, 0.2);
        assert_eq!(left_a, left_b, "the same main signal drives both engines");

        // A full-scale, rapidly changing sidechain would be very audible if it leaked.
        let mut ref_left = vec![0.0f32; BLOCK];
        let mut ref_right = vec![0.0f32; BLOCK];
        for i in 0..BLOCK {
            noise_phase = (noise_phase + 0.71) % TAU;
            ref_left[i] = noise_phase.sin();
            ref_right[i] = -noise_phase.sin();
        }

        let mut channels_a: Vec<&mut [f32]> = vec![&mut left_a, &mut right_a];
        let mut sidechain: Vec<&mut [f32]> = vec![&mut ref_left, &mut ref_right];
        with_reference
            .engine
            .process(&mut channels_a, &mut sidechain, false);

        let mut channels_b: Vec<&mut [f32]> = vec![&mut left_b, &mut right_b];
        let mut no_sidechain: Vec<&mut [f32]> = Vec::new();
        without_reference
            .engine
            .process(&mut channels_b, &mut no_sidechain, false);

        assert_eq!(
            &*channels_a[0], &*channels_b[0],
            "the sidechain changed the left output in block {block}"
        );
        assert_eq!(
            &*channels_a[1], &*channels_b[1],
            "the sidechain changed the right output in block {block}"
        );
    }

    // The reference was actually measured, so the comparison above is not vacuous.
    let measured = with_reference
        .shared
        .reference
        .integrated
        .load()
        .expect("the sidechain programme was measured");
    assert!(measured > -10.0, "{measured}");
    assert!(without_reference.shared.deltas().integrated.is_none());
}

#[test]
fn the_deltas_follow_known_level_differences() {
    let mut both = rig(2, 2);
    // -12 dBFS and -18 dBFS: the same waveform 6.02 dB apart.
    run(
        &mut both,
        (RATE * 4.0) as usize,
        [0.25, 0.25],
        [0.125, 0.125],
        2,
    );
    let deltas = both.shared.deltas();
    for (name, value) in [
        ("momentary", deltas.momentary),
        ("short term", deltas.short_term),
        ("integrated", deltas.integrated),
    ] {
        let value = value.unwrap_or_else(|| panic!("{name} delta is missing"));
        assert!(
            (value - 6.02).abs() < 0.15,
            "{name} delta {value} LU is not 6.02 LU"
        );
    }

    // The same programme on both inputs compares as 0 LU, also in a mono layout.
    let mut mono = rig(1, 1);
    run(
        &mut mono,
        (RATE * 4.0) as usize,
        [0.125, 0.125],
        [0.125, 0.125],
        1,
    );
    let integrated = mono
        .shared
        .deltas()
        .integrated
        .expect("mono comparison is available");
    assert!(integrated.abs() < 0.05, "{integrated}");

    // A quieter reference reads as a positive boost.
    let mut quieter = rig(1, 1);
    run(
        &mut quieter,
        (RATE * 4.0) as usize,
        [0.25, 0.25],
        [0.0625, 0.0625],
        1,
    );
    let integrated = quieter
        .shared
        .deltas()
        .integrated
        .expect("comparison is available");
    assert!((integrated - 12.04).abs() < 0.15, "{integrated}");
}

#[test]
fn an_absent_reference_reads_as_unavailable() {
    // No sidechain in the layout at all.
    let mut none = rig(2, 0);
    run(
        &mut none,
        (RATE * 2.0) as usize,
        [0.25, 0.25],
        [0.0, 0.0],
        0,
    );
    assert!(!none.shared.reference_available());
    let deltas = none.shared.deltas();
    assert!(deltas.momentary.is_none() && deltas.short_term.is_none());
    assert!(deltas.integrated.is_none());
    assert_eq!(shirabe_ui::format_delta_lu(deltas.integrated), "--");

    // A sidechain input that carries silence is not a reference: -inf is not a measurement,
    // so the delta stays unavailable instead of reporting a huge positive boost.
    let mut silent = rig(2, 2);
    run(
        &mut silent,
        (RATE * 2.0) as usize,
        [0.25, 0.25],
        [0.0, 0.0],
        2,
    );
    assert!(silent.shared.reference_available());
    assert!(silent.shared.reference.momentary.load().is_some());
    let deltas = silent.shared.deltas();
    assert!(deltas.momentary.is_none(), "{deltas:?}");
    assert!(deltas.short_term.is_none(), "{deltas:?}");
    assert!(deltas.integrated.is_none(), "{deltas:?}");
    assert_eq!(shirabe_ui::format_delta_lu(deltas.momentary), "--");

    // The main measurement itself is unaffected by the silent sidechain: a 1 kHz stereo sine
    // at -12 dBFS peak reads -12.0 LUFS, as documented for the calibration tone.
    let main = silent
        .shared
        .loudness
        .integrated
        .load()
        .expect("main programme");
    assert!((main + 12.0).abs() < 0.1, "{main}");
}

#[test]
fn reset_and_pause_follow_the_main_measurement() {
    let mut rig = rig(2, 2);
    let frames = (RATE * 2.0) as usize;
    run(&mut rig, frames, [0.25, 0.25], [0.125, 0.125], 2);
    let reference = rig
        .shared
        .reference
        .integrated
        .load()
        .expect("reference programme");
    let main = rig
        .shared
        .loudness
        .integrated
        .load()
        .expect("main programme");

    // Pause freezes the integrated values of both measurements, while M / S keep running.
    rig.shared.set_integrating(false);
    run(&mut rig, frames, [0.0, 0.0], [0.0, 0.0], 2);
    assert_eq!(rig.shared.reference.integrated.load(), Some(reference));
    assert_eq!(rig.shared.loudness.integrated.load(), Some(main));
    assert!(rig.shared.reference.momentary.load().is_some());

    // Continue picks up again.
    rig.shared.set_integrating(true);
    run(&mut rig, frames, [0.25, 0.25], [0.125, 0.125], 2);
    assert!(rig.shared.reference.integrated.load().is_some());

    // Reset clears the reference programme together with the main one.
    rig.shared.request_reset();
    run(&mut rig, BLOCK, [0.25, 0.25], [0.125, 0.125], 2);
    assert!(rig.shared.reference.integrated.load().is_none());
    assert!(rig.shared.loudness.integrated.load().is_none());
}

#[test]
fn a_silent_reference_does_not_zero_the_comparison() {
    // Guards the display rule directly: no finite value on either side means no delta.
    let rig = rig(2, 2);
    rig.shared
        .reference
        .momentary
        .store(Some(f64::NEG_INFINITY));
    rig.shared.loudness.momentary.store(Some(-14.0));
    assert!(rig.shared.deltas().momentary.is_none());
    rig.shared.reference.momentary.store(Some(-20.0));
    let delta = rig.shared.deltas().momentary.expect("delta");
    assert!((delta - 6.0).abs() < 1e-4, "{delta}");
}

#[test]
fn the_sidechain_layouts_only_append_to_the_existing_buses() {
    let layouts = <Shirabe as Plugin>::AUDIO_IO_LAYOUTS;
    assert_eq!(layouts.len(), 4, "mono, stereo and two sidechain layouts");
    // The first two entries are the layouts that shipped before the sidechain existed. Their
    // numbers must not change, or existing projects get a different bus configuration.
    assert_eq!(layouts[0].main_input_channels.map(|c| c.get()), Some(2));
    assert_eq!(layouts[0].main_output_channels.map(|c| c.get()), Some(2));
    assert!(layouts[0].aux_input_ports.is_empty());
    assert!(layouts[0].aux_output_ports.is_empty());
    assert_eq!(layouts[1].main_input_channels.map(|c| c.get()), Some(1));
    assert_eq!(layouts[1].main_output_channels.map(|c| c.get()), Some(1));
    assert!(layouts[1].aux_input_ports.is_empty());

    // The sidechain layouts are appended: stereo main with a stereo reference, mono with mono.
    assert_eq!(layouts[2].main_input_channels.map(|c| c.get()), Some(2));
    assert_eq!(layouts[2].main_output_channels.map(|c| c.get()), Some(2));
    assert_eq!(
        layouts[2]
            .aux_input_ports
            .iter()
            .map(|c| c.get())
            .collect::<Vec<_>>(),
        vec![2]
    );
    assert_eq!(layouts[3].main_input_channels.map(|c| c.get()), Some(1));
    assert_eq!(
        layouts[3]
            .aux_input_ports
            .iter()
            .map(|c| c.get())
            .collect::<Vec<_>>(),
        vec![1]
    );
    // No new output: the plug-in stays pass-through on the main bus only.
    for layout in layouts {
        assert!(layout.aux_output_ports.is_empty());
    }
}

#[test]
fn the_sidechain_path_does_not_allocate_on_the_audio_thread() {
    let mut rig = rig(2, 2);
    let mut left = vec![0.25f32; 512];
    let mut right = vec![0.25f32; 512];
    let mut ref_left = vec![0.25f32; 512];
    let mut ref_right = vec![0.25f32; 512];
    let mut channels: Vec<&mut [f32]> = vec![&mut left, &mut right];
    let mut sidechain: Vec<&mut [f32]> = vec![&mut ref_left, &mut ref_right];
    assert_no_alloc::assert_no_alloc(|| {
        rig.engine.process(&mut channels, &mut sidechain, false);
    });
    // And the layout change that allocates runs outside the audio callback.
    rig.engine.prepare(RATE, 2, 2);
    assert!(rig.shared.reference_available());
}

/// Measures what the reference measurement costs on this machine. Not a correctness test, so it
/// is ignored by default and run by hand for the documented figure:
///
/// ```sh
/// cargo test --release -p shirabe-plugin --test reference_comparison -- --ignored --nocapture
/// ```
#[test]
#[ignore = "timing; run with --release for the documented CPU figure"]
fn the_reference_costs_about_one_more_loudness_measurement() {
    const SECONDS: usize = 20;
    const REPEATS: usize = 5;
    let frames = (RATE as usize) * SECONDS;
    let blocks = frames / BLOCK;

    let measure = |reference_channels: usize| -> Vec<f64> {
        let mut samples = Vec::new();
        for _ in 0..REPEATS {
            let mut rig = rig(2, reference_channels);
            let mut tone = Tone::new(1000.0);
            let mut left = vec![0.0f32; BLOCK];
            let mut right = vec![0.0f32; BLOCK];
            let mut ref_left = vec![0.0f32; BLOCK];
            let mut ref_right = vec![0.0f32; BLOCK];
            tone.fill(&mut left, 0.2);
            tone.fill(&mut right, 0.2);
            tone.fill(&mut ref_left, 0.2);
            tone.fill(&mut ref_right, 0.2);
            let mut channels: Vec<&mut [f32]> = vec![&mut left, &mut right];
            let mut sidechain: Vec<&mut [f32]> = match reference_channels {
                0 => Vec::new(),
                _ => vec![&mut ref_left, &mut ref_right],
            };
            let start = std::time::Instant::now();
            for _ in 0..blocks {
                rig.engine.process(&mut channels, &mut sidechain, false);
            }
            samples.push(start.elapsed().as_secs_f64() / frames as f64);
        }
        samples.sort_by(f64::total_cmp);
        samples
    };

    let without = measure(0);
    let with = measure(2);
    let budget = 1.0 / RATE as f64;
    let median = |samples: &[f64]| samples[samples.len() / 2];
    println!("without reference (us/frame): {without:?}");
    println!("with reference    (us/frame): {with:?}");
    let base = median(&without);
    let added = median(&with);
    println!(
        "median: {:.3} -> {:.3} us/frame, reference adds {:.3} us/frame = {:.1}% of one core at 48 kHz ({:.2}x)",
        base * 1e6,
        added * 1e6,
        (added - base) * 1e6,
        (added - base) / budget * 100.0,
        added / base
    );
    // A second loudness meter cannot cost more than the whole engine again.
    assert!(added < base * 2.5, "{added} vs {base}");
}
