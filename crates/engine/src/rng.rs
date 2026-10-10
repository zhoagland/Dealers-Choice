//! Deterministic randomness. Implements PRD NFR-1 and `docs/tech/engine.md` §7.
//!
//! Rule: **all** randomness in game logic, bots, and sims comes from a [`DetRng`] passed in
//! explicitly. Never call `rand::rng()` / `thread_rng()`; it is seeded from the OS, so results
//! couldn't be reproduced.

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

/// The engine's RNG: ChaCha with 8 rounds.
///
/// Why ChaCha8: it's a cryptographic-quality stream cipher (no detectable statistical bias for
/// simulations), it's fast (on the order of a few GB/s), and its output for a given seed is
/// specified by the algorithm rather than the platform, so seeds replay the same everywhere.
pub type DetRng = ChaCha8Rng;

/// Which consumer an RNG is for. Separate streams mean, for example, that a bot drawing more
/// random numbers (a bigger search budget) can't change the cards that get dealt.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Stream {
    /// Chance events: shuffling and dealing.
    Deal,
    /// The bot sitting in this seat.
    Seat(u8),
    /// Anything else (coach, spot analysis…), numbered by the caller.
    Custom(u32),
}

impl Stream {
    /// A distinct number per stream. The ranges can't overlap: Deal = 0, Seat = 1..=256,
    /// Custom = 2^32 and up.
    fn id(self) -> u64 {
        match self {
            Stream::Deal => 0,
            Stream::Seat(s) => 1 + s as u64,
            Stream::Custom(n) => (1u64 << 32) | n as u64,
        }
    }
}

/// An RNG from a single number, for tests and simple tools.
pub fn rng_from_seed(seed: u64) -> DetRng {
    DetRng::seed_from_u64(seed)
}

/// The RNG for one consumer in one hand of a run.
///
/// The ChaCha key is literally `(run_seed, hand_index, stream)` laid out in its 32 bytes, so
/// different inputs give different keys, and ChaCha turns different keys into streams with no
/// detectable relationship. Hand `i` therefore gets the same randomness regardless of which
/// thread plays it or in what order, which is what makes parallel sims reproducible.
pub fn derive_rng(run_seed: u64, hand_index: u64, stream: Stream) -> DetRng {
    let mut seed = [0u8; 32];
    seed[0..8].copy_from_slice(&run_seed.to_le_bytes());
    seed[8..16].copy_from_slice(&hand_index.to_le_bytes());
    seed[16..24].copy_from_slice(&stream.id().to_le_bytes());
    // Bytes 24..32 stay zero. They're reserved for future use (e.g. a session index).
    DetRng::from_seed(seed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::Rng;

    /// First few outputs, enough to tell streams apart.
    fn sample(mut rng: DetRng) -> [u64; 4] {
        [rng.random(), rng.random(), rng.random(), rng.random()]
    }

    #[test]
    fn same_inputs_same_stream() {
        assert_eq!(
            sample(derive_rng(1, 2, Stream::Deal)),
            sample(derive_rng(1, 2, Stream::Deal))
        );
    }

    #[test]
    fn any_input_change_gives_a_different_stream() {
        let base = sample(derive_rng(1, 2, Stream::Deal));
        assert_ne!(base, sample(derive_rng(9, 2, Stream::Deal)));
        assert_ne!(base, sample(derive_rng(1, 9, Stream::Deal)));
        assert_ne!(base, sample(derive_rng(1, 2, Stream::Seat(0))));
        assert_ne!(
            sample(derive_rng(1, 2, Stream::Seat(0))),
            sample(derive_rng(1, 2, Stream::Seat(1)))
        );
        assert_ne!(base, sample(derive_rng(1, 2, Stream::Custom(0))));
    }

    #[test]
    fn stream_ids_are_distinct() {
        let mut ids: Vec<u64> = (0..=u8::MAX).map(|s| Stream::Seat(s).id()).collect();
        ids.push(Stream::Deal.id());
        ids.push(Stream::Custom(0).id());
        ids.push(Stream::Custom(u32::MAX).id());
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }

    /// Golden value: pins the exact output for a fixed seed. If this ever fails, a dependency
    /// upgrade changed the random stream and saved seeds no longer replay. That needs a
    /// deliberate decision (version bump), not a silent update.
    #[test]
    fn golden_output_is_stable() {
        let mut rng = derive_rng(42, 0, Stream::Deal);
        let first: u64 = rng.random();
        assert_eq!(first, GOLDEN_FIRST_U64);
    }

    // Recorded from the first run. See `golden_output_is_stable`.
    const GOLDEN_FIRST_U64: u64 = 6_424_161_053_832_095_879;
}
