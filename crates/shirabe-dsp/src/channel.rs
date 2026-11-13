//! Channel layouts and the role of each channel.
//!
//! Meters that weight or group channels (loudness, surround displays) look at the role of a
//! channel instead of its index, so new layouts only need a new entry here.

/// What a channel carries in a layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelRole {
    /// Mono.
    Mono,
    /// Front left.
    Left,
    /// Front right.
    Right,
    /// Front centre.
    Centre,
    /// Low frequency effects.
    Lfe,
    /// Left surround.
    LeftSurround,
    /// Right surround.
    RightSurround,
}

impl ChannelRole {
    /// Short label for meter captions.
    pub fn label(self) -> &'static str {
        match self {
            ChannelRole::Mono => "M",
            ChannelRole::Left => "L",
            ChannelRole::Right => "R",
            ChannelRole::Centre => "C",
            ChannelRole::Lfe => "LFE",
            ChannelRole::LeftSurround => "Ls",
            ChannelRole::RightSurround => "Rs",
        }
    }

    /// Channel weighting for programme loudness (ITU-R BS.1770): surround channels get
    /// +1.5 dB (power weight 1.41), LFE is excluded.
    pub fn loudness_weight(self) -> f32 {
        match self {
            ChannelRole::Lfe => 0.0,
            ChannelRole::LeftSurround | ChannelRole::RightSurround => 1.41,
            _ => 1.0,
        }
    }
}

/// A supported channel layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelLayout {
    /// One channel.
    Mono,
    /// Left and right.
    Stereo,
    /// L, R, C, LFE, Ls, Rs (the usual 5.1 interleaving order).
    Surround51,
}

impl ChannelLayout {
    /// Picks the layout for a channel count, if supported.
    pub fn from_channel_count(count: usize) -> Option<Self> {
        match count {
            1 => Some(Self::Mono),
            2 => Some(Self::Stereo),
            6 => Some(Self::Surround51),
            _ => None,
        }
    }

    /// The roles in channel order.
    pub fn roles(self) -> &'static [ChannelRole] {
        use ChannelRole::*;
        match self {
            Self::Mono => &[Mono],
            Self::Stereo => &[Left, Right],
            Self::Surround51 => &[Left, Right, Centre, Lfe, LeftSurround, RightSurround],
        }
    }

    /// Number of channels.
    pub fn channel_count(self) -> usize {
        self.roles().len()
    }

    /// Human readable name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Mono => "Mono",
            Self::Stereo => "Stereo",
            Self::Surround51 => "5.1",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_round_trip() {
        for count in [1, 2, 6] {
            let layout = ChannelLayout::from_channel_count(count).unwrap();
            assert_eq!(layout.channel_count(), count);
        }
        assert_eq!(ChannelLayout::from_channel_count(3), None);
    }

    #[test]
    fn lfe_is_excluded_from_loudness() {
        let weights: Vec<f32> = ChannelLayout::Surround51
            .roles()
            .iter()
            .map(|r| r.loudness_weight())
            .collect();
        assert_eq!(weights, vec![1.0, 1.0, 1.0, 0.0, 1.41, 1.41]);
    }
}
