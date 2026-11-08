//! Timing that works in the browser too: `std::time::Instant` panics on
//! `wasm32-unknown-unknown`, so there elapsed times read as zero and the
//! page measures around its calls instead.

use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
pub struct Stopwatch(std::time::Instant);

#[cfg(target_arch = "wasm32")]
pub struct Stopwatch;

impl Stopwatch {
    pub fn start() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        return Self(std::time::Instant::now());
        #[cfg(target_arch = "wasm32")]
        return Self;
    }

    pub fn elapsed(&self) -> Duration {
        #[cfg(not(target_arch = "wasm32"))]
        return self.0.elapsed();
        #[cfg(target_arch = "wasm32")]
        return Duration::ZERO;
    }
}
