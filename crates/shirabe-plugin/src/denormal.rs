//! Flush-to-zero / denormals-are-zero for the duration of a processing call.
//!
//! Tiny (denormal) input values can make the meter filters many times slower on x86. Most
//! hosts already enable these modes, but not all do; the guard enables them and restores the
//! previous state afterwards so the host is not affected.

/// Restores the previous floating-point mode when dropped.
pub struct DenormalGuard {
    #[cfg(target_arch = "x86_64")]
    previous: u32,
}

impl DenormalGuard {
    /// Enables flush-to-zero and denormals-are-zero on the current thread.
    #[inline]
    pub fn new() -> Self {
        #[cfg(target_arch = "x86_64")]
        {
            // SSE is always available on x86_64. The intrinsics are deprecated in favour of
            // inline assembly, but remain the simplest correct way to do this.
            #[allow(deprecated)]
            unsafe {
                use std::arch::x86_64::{_mm_getcsr, _mm_setcsr};
                const FTZ_DAZ: u32 = 0x8040;
                let previous = _mm_getcsr();
                _mm_setcsr(previous | FTZ_DAZ);
                Self { previous }
            }
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            Self {}
        }
    }
}

impl Default for DenormalGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for DenormalGuard {
    #[inline]
    fn drop(&mut self) {
        #[cfg(target_arch = "x86_64")]
        #[allow(deprecated)]
        unsafe {
            std::arch::x86_64::_mm_setcsr(self.previous);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_arch = "x86_64")]
    fn flushes_denormals_and_restores() {
        let tiny = std::hint::black_box(f32::MIN_POSITIVE);
        {
            let _guard = DenormalGuard::new();
            // Halving the smallest normal gives a denormal, which is flushed to zero.
            assert_eq!(std::hint::black_box(tiny) / 2.0, 0.0);
        }
        assert!(std::hint::black_box(tiny) / 2.0 > 0.0);
    }
}
