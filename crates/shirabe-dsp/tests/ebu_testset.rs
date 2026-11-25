//! Conformance tests with the official EBU loudness test set (EBU Tech 3341 / 3342).
//!
//! The test set may only be used for technical testing and must not be redistributed, so it is
//! not part of this repository. Download `ebu-loudness-test-setv05.zip` from the EBU, extract it
//! and point `SHIRABE_EBU_TESTSET` at the directory, then run:
//!
//! ```text
//! SHIRABE_EBU_TESTSET=/path/to/testset cargo test --release -p shirabe-dsp --test ebu_testset -- --ignored
//! ```
//!
//! Expected values and tolerances are those of EBU Tech 3341 (2023) Table 1 and
//! EBU Tech 3342 (2023) Table 1.

use std::path::PathBuf;

use shirabe_dsp::{ChannelLayout, ChannelRole, LoudnessMeter, TruePeakMeter};

fn testset_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("SHIRABE_EBU_TESTSET")
            .expect("set SHIRABE_EBU_TESTSET to the extracted EBU loudness test set"),
    )
}

struct Audio {
    sample_rate: f32,
    channels: Vec<Vec<f32>>,
}

fn load(name: &str) -> Audio {
    let path = testset_dir().join(name);
    let mut reader =
        hound::WavReader::open(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let spec = reader.spec();
    let n = spec.channels as usize;
    let mut channels = vec![Vec::new(); n];
    match spec.sample_format {
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
            for (i, s) in reader.samples::<i32>().enumerate() {
                channels[i % n].push(s.unwrap() as f32 * scale);
            }
        }
        hound::SampleFormat::Float => {
            for (i, s) in reader.samples::<f32>().enumerate() {
                channels[i % n].push(s.unwrap());
            }
        }
    }
    Audio {
        sample_rate: spec.sample_rate as f32,
        channels,
    }
}

fn stereo_roles() -> &'static [ChannelRole] {
    ChannelLayout::Stereo.roles()
}

/// Feeds the audio in 10 ms blocks and calls `each` after every block with the elapsed time.
fn run(meter: &mut LoudnessMeter, audio: &Audio, mut each: impl FnMut(f64, &LoudnessMeter)) {
    let block = (audio.sample_rate / 100.0) as usize;
    let len = audio.channels[0].len();
    let mut i = 0;
    while i < len {
        let end = (i + block).min(len);
        let chunk: Vec<&[f32]> = audio.channels.iter().map(|c| &c[i..end]).collect();
        meter.process(&chunk);
        each(end as f64 / audio.sample_rate as f64, meter);
        i = end;
    }
}

fn integrated(name: &str, roles: &[ChannelRole]) -> f64 {
    let audio = load(name);
    let mut meter = LoudnessMeter::new(audio.sample_rate, roles);
    run(&mut meter, &audio, |_, _| {});
    meter.readout().integrated.expect("no integrated value")
}

fn check(name: &str, what: &str, value: f64, expected: f64, minus: f64, plus: f64) {
    let ok = value >= expected - minus && value <= expected + plus;
    println!(
        "{} {name:<44} {what:<6} {value:>8.3} (expected {expected} +{plus}/-{minus})",
        if ok { "ok  " } else { "FAIL" }
    );
    assert!(
        ok,
        "{name} {what}: {value:.3}, expected {expected} +{plus}/-{minus}"
    );
}

#[test]
#[ignore = "needs the EBU loudness test set (SHIRABE_EBU_TESTSET)"]
fn tech3341_cases_1_and_2() {
    for (name, expected) in [
        ("seq-3341-1-16bit.wav", -23.0),
        ("seq-3341-2-16bit.wav", -33.0),
    ] {
        let audio = load(name);
        let mut meter = LoudnessMeter::new(audio.sample_rate, stereo_roles());
        // M and S must read the level once their windows are filled.
        run(&mut meter, &audio, |t, m| {
            let r = m.readout();
            if t > 0.5 {
                check(name, "M", r.momentary.unwrap(), expected, 0.1, 0.1);
            }
            if t > 3.1 {
                check(name, "S", r.short_term.unwrap(), expected, 0.1, 0.1);
            }
        });
        check(
            name,
            "I",
            meter.readout().integrated.unwrap(),
            expected,
            0.1,
            0.1,
        );
    }
}

#[test]
#[ignore = "needs the EBU loudness test set (SHIRABE_EBU_TESTSET)"]
fn tech3341_cases_3_to_8_integrated() {
    use ChannelRole::*;
    let surround = [Left, Right, Centre, LeftSurround, RightSurround];
    let cases: [(&str, &[ChannelRole]); 6] = [
        ("seq-3341-3-16bit-v02.wav", stereo_roles()),
        ("seq-3341-4-16bit-v02.wav", stereo_roles()),
        ("seq-3341-5-16bit-v02.wav", stereo_roles()),
        ("seq-3341-6-5channels-16bit.wav", &surround),
        ("seq-3341-7_seq-3342-5-24bit.wav", stereo_roles()),
        ("seq-3341-2011-8_seq-3342-6-24bit-v02.wav", stereo_roles()),
    ];
    for (name, roles) in cases {
        check(name, "I", integrated(name, roles), -23.0, 0.1, 0.1);
    }
}

#[test]
#[ignore = "needs the EBU loudness test set (SHIRABE_EBU_TESTSET)"]
fn tech3341_case9_short_term_constant() {
    let name = "seq-3341-9-24bit.wav";
    let audio = load(name);
    let mut meter = LoudnessMeter::new(audio.sample_rate, stereo_roles());
    run(&mut meter, &audio, |t, m| {
        if t >= 3.0 {
            check(name, "S", m.readout().short_term.unwrap(), -23.0, 0.1, 0.1);
        }
    });
}

#[test]
#[ignore = "needs the EBU loudness test set (SHIRABE_EBU_TESTSET)"]
fn tech3341_case10_max_short_term_per_segment() {
    for i in 1..=20 {
        let name = format!("seq-3341-10-{i}-24bit.wav");
        let audio = load(&name);
        let mut meter = LoudnessMeter::new(audio.sample_rate, stereo_roles());
        run(&mut meter, &audio, |_, _| {});
        let max = meter.readout().max_short_term.unwrap();
        check(&name, "Max S", max, -23.0, 0.1, 0.1);
    }
}

#[test]
#[ignore = "needs the EBU loudness test set (SHIRABE_EBU_TESTSET)"]
fn tech3341_case11_max_short_term_live() {
    let name = "seq-3341-11-24bit.wav";
    let audio = load(name);
    let mut meter = LoudnessMeter::new(audio.sample_rate, stereo_roles());
    // Segment i lasts i*0.15 + 3 + (3 - i*0.15) = 6 s; read Max S at the end of each segment.
    let mut next = 0;
    run(&mut meter, &audio, |t, m| {
        if next < 20 && t >= (next + 1) as f64 * 6.0 - 1e-9 {
            let expected = -38.0 + next as f64;
            check(
                name,
                "Max S",
                m.readout().max_short_term.unwrap(),
                expected,
                0.1,
                0.1,
            );
            next += 1;
        }
    });
    assert_eq!(next, 20);
}

#[test]
#[ignore = "needs the EBU loudness test set (SHIRABE_EBU_TESTSET)"]
fn tech3341_case12_momentary_constant() {
    let name = "seq-3341-12-24bit.wav";
    let audio = load(name);
    let mut meter = LoudnessMeter::new(audio.sample_rate, stereo_roles());
    run(&mut meter, &audio, |t, m| {
        if t >= 1.0 {
            check(name, "M", m.readout().momentary.unwrap(), -23.0, 0.1, 0.1);
        }
    });
}

#[test]
#[ignore = "needs the EBU loudness test set (SHIRABE_EBU_TESTSET)"]
fn tech3341_case13_max_momentary_per_segment() {
    for i in 1..=20 {
        // The test set names most of these files with a doubled extension.
        let plain = format!("seq-3341-13-{i}-24bit.wav");
        let name = if testset_dir().join(&plain).exists() {
            plain
        } else {
            format!("{plain}.wav")
        };
        let audio = load(&name);
        let mut meter = LoudnessMeter::new(audio.sample_rate, stereo_roles());
        run(&mut meter, &audio, |_, _| {});
        let max = meter.readout().max_momentary.unwrap();
        check(&name, "Max M", max, -23.0, 0.1, 0.1);
    }
}

#[test]
#[ignore = "needs the EBU loudness test set (SHIRABE_EBU_TESTSET)"]
fn tech3341_case14_max_momentary_live() {
    let name = "seq-3341-14-24bit.wav.wav";
    let audio = load(name);
    let mut meter = LoudnessMeter::new(audio.sample_rate, stereo_roles());
    // Segment i lasts i*0.02 + 0.4 + (0.4 - i*0.02) = 0.8 s.
    let mut next = 0;
    run(&mut meter, &audio, |t, m| {
        if next < 20 && t >= (next + 1) as f64 * 0.8 - 1e-9 {
            let expected = -38.0 + next as f64;
            check(
                name,
                "Max M",
                m.readout().max_momentary.unwrap(),
                expected,
                0.1,
                0.1,
            );
            next += 1;
        }
    });
    assert_eq!(next, 20);
}

#[test]
#[ignore = "needs the EBU loudness test set (SHIRABE_EBU_TESTSET)"]
fn tech3341_cases_15_to_23_true_peak() {
    let cases = [
        (15, -6.0),
        (16, -6.0),
        (17, -6.0),
        (18, -6.0),
        (19, 3.0),
        (20, 0.0),
        (21, 0.0),
        (22, 0.0),
        (23, 0.0),
    ];
    for (case, expected) in cases {
        let name = format!("seq-3341-{case}-24bit.wav.wav");
        let audio = load(&name);
        let mut max = f32::NEG_INFINITY;
        for channel in &audio.channels {
            let mut tp = TruePeakMeter::new(audio.sample_rate);
            for chunk in channel.chunks(480) {
                tp.process(chunk);
            }
            max = max.max(tp.max_dbtp());
        }
        check(&name, "TP", max as f64, expected, 0.4, 0.2);
    }
}

#[test]
#[ignore = "needs the EBU loudness test set (SHIRABE_EBU_TESTSET)"]
fn tech3342_cases_1_to_6_loudness_range() {
    let cases = [
        ("seq-3342-1-16bit.wav", 10.0),
        ("seq-3342-2-16bit.wav", 5.0),
        ("seq-3342-3-16bit.wav", 20.0),
        ("seq-3342-4-16bit.wav", 15.0),
        ("seq-3341-7_seq-3342-5-24bit.wav", 5.0),
        ("seq-3341-2011-8_seq-3342-6-24bit-v02.wav", 15.0),
    ];
    for (name, expected) in cases {
        let audio = load(name);
        let mut meter = LoudnessMeter::new(audio.sample_rate, stereo_roles());
        run(&mut meter, &audio, |_, _| {});
        check(
            name,
            "LRA",
            meter.readout().range.unwrap(),
            expected,
            1.0,
            1.0,
        );
    }
}
