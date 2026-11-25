//! Real-time audio measurement primitives for the Shirabe meter suite.
//!
//! Everything in this crate is framework independent: no plugin API, no GUI toolkit and no
//! allocation in the per-block processing paths. Objects are created and resized on a non
//! real-time thread (`new`, `prepare`), then fed with audio blocks on the audio thread.
//!
//! Licensed under either of MIT or Apache-2.0 at your option.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod biquad;
pub mod channel;
pub mod kweighting;
pub mod level;
pub mod loudness;
pub mod osc;
pub mod peak;
pub mod ppm;
pub mod true_peak;
pub mod vu;

pub use channel::{ChannelLayout, ChannelRole};
pub use level::{db_to_gain, gain_to_db, MINUS_INFINITY_DB};
pub use loudness::{LoudnessMeter, LoudnessReadout};
pub use osc::SineOscillator;
pub use peak::{PeakBallistics, PeakMeter};
pub use ppm::{PpmMeter, PpmType};
pub use true_peak::TruePeakMeter;
pub use vu::VuMeter;
