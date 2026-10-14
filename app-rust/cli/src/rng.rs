//! Hand-rolled PCG32 (XSH-RR, setseq 64/64): the only source of
//! randomness in the product, used to pick one of the eight waiting
//! animations per download run.
//!
//! Algorithm and seeding follow the canonical minimal edition,
//! `pcg_basic.c` from github.com/imneme/pcg-c-basic; the known-answer
//! values in the tests come from compiling that source and running it
//! (seed 42, seq 54) on 2026-10-14, so they are the reference's own
//! output, not this port's.

pub struct Pcg32 {
    state: u64,
    inc: u64,
}

impl Pcg32 {
    /// Two-part seed, exactly the reference `pcg32_srandom_r`.
    pub fn new(initstate: u64, initseq: u64) -> Self {
        let mut rng = Pcg32 {
            state: 0,
            inc: (initseq << 1) | 1,
        };
        rng.next_u32();
        rng.state = rng.state.wrapping_add(initstate);
        rng.next_u32();
        rng
    }

    /// The reference `pcg32_random_r`.
    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(6364136223846793005).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        (xorshifted >> rot) | (xorshifted << (rot.wrapping_neg() & 31))
    }

    /// Uniform in `0..bound` via the reference threshold scheme
    /// (`pcg32_boundedrand_r`), so no modulo bias.
    pub fn below(&mut self, bound: u32) -> u32 {
        assert!(bound > 0, "bound must be positive");
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let r = self.next_u32();
            if r >= threshold {
                return r % bound;
            }
        }
    }
}

/// A per-process seed for the product: wall clock in nanoseconds mixed
/// with the process id. Not cryptographic; the only consumer is which
/// animation spins.
pub fn seeded() -> Pcg32 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    let pid = u64::from(std::process::id());
    Pcg32::new(nanos ^ pid.rotate_left(32), 0x14057b7ef767814f)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference output of the compiled upstream pcg-c-basic, seed 42,
    /// seq 54, run 2026-10-14: eight raw draws, then twelve `bounded(8)`
    /// draws, then sixteen `bounded(2)` draws, in that call order.
    #[test]
    fn matches_reference_vectors() {
        let mut rng = Pcg32::new(42, 54);
        let raw: Vec<u32> = (0..8).map(|_| rng.next_u32()).collect();
        assert_eq!(
            raw,
            [
                0xa15c02b7, 0x7b47f409, 0xba1d3330, 0x83d2f293, 0xbfa4784b, 0xcbed606e, 0xbfc6a3ad,
                0x812fff6d
            ]
        );
        let b8: Vec<u32> = (0..12).map(|_| rng.below(8)).collect();
        assert_eq!(b8, [2, 0, 6, 1, 6, 5, 7, 3, 2, 3, 2, 6]);
        let b2: Vec<u32> = (0..16).map(|_| rng.below(2)).collect();
        assert_eq!(b2, [0, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 1, 0, 1]);
    }

    #[test]
    fn bounded_draws_stay_in_range() {
        let mut rng = seeded();
        for _ in 0..1000 {
            assert!(rng.below(8) < 8);
        }
    }
}
