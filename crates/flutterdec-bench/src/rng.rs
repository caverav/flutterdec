//! Seeded, dependency-free randomness for the held-out generator.
//!
//! SplitMix64, whose state is derived from the recorded 128-bit seed by hashing
//! it. Hashing rather than taking the low half means every bit of the recorded
//! seed reaches the stream, so two seeds that differ only above bit 63 do not
//! generate the same workload.

use crate::sha256::Sha256;

pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn from_seed(seed: u128) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"flutterdec-bench/held-out/v1");
        hasher.update(&seed.to_be_bytes());
        let digest = hasher.finish();
        let mut first = [0u8; 8];
        first.copy_from_slice(&digest[..8]);
        Self {
            state: u64::from_be_bytes(first),
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }

    /// Uniform on `0..n`. Rejection sampling rather than a plain modulo: the
    /// contract says the held-out block size is selected uniformly, and a
    /// modulo over a range that does not divide 2^64 is measurably biased
    /// toward its low end.
    pub fn below(&mut self, n: u64) -> u64 {
        assert!(n > 0, "range must be non-empty");
        let limit = u64::MAX - (u64::MAX % n);
        loop {
            let value = self.next_u64();
            if value < limit {
                return value % n;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole held-out protocol rests on a recorded seed reproducing the
    /// same cases later. A stream that drifted would make every recorded seed
    /// worthless.
    #[test]
    fn the_same_seed_reproduces_the_same_stream() {
        let seed = 0x0123_4567_89ab_cdef_fedc_ba98_7654_3210u128;
        let a: Vec<u64> = (0..32).map(|_| Rng::from_seed(seed).next_u64()).collect();
        assert!(a.windows(2).all(|w| w[0] == w[1]), "fresh seeds agree");

        let mut first = Rng::from_seed(seed);
        let mut second = Rng::from_seed(seed);
        for _ in 0..1000 {
            assert_eq!(first.next_u64(), second.next_u64());
        }
    }

    /// A seed differing only in its high 64 bits must not produce the same
    /// workload, which is what taking the low half of the seed would do.
    #[test]
    fn the_high_half_of_the_seed_reaches_the_stream() {
        let low = Rng::from_seed(1).next_u64();
        let high = Rng::from_seed(1 | (1u128 << 100)).next_u64();
        assert_ne!(low, high);
    }

    /// Uniformity is a contract term, not a nicety: the held-out block size is
    /// drawn from a range of about 1950 values and a biased draw would keep
    /// picking the small, cheap graphs.
    #[test]
    fn below_is_within_range_and_roughly_uniform() {
        let mut rng = Rng::from_seed(0xdead_beefu128);
        let buckets = 8u64;
        let mut counts = [0usize; 8];
        let draws = 80_000;
        for _ in 0..draws {
            let value = rng.below(buckets);
            assert!(value < buckets);
            counts[value as usize] += 1;
        }
        let expected = draws / buckets as usize;
        for count in counts {
            let spread = (count as i64 - expected as i64).unsigned_abs();
            assert!(
                spread * 20 < expected as u64,
                "bucket {count} is more than 5 percent off {expected}"
            );
        }
    }
}
