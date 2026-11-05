//! Mersenne Twister random number generator, ported from upstream
//! `Core/SoarKernel/src/shared/soar_rand.h` (`MTRand`, MT19937), so a seed reproduces
//! upstream's random choices.

const N: usize = 624;
const M: usize = 397;

/// Upstream `MTRand`.
#[derive(Clone)]
pub struct MtRand {
    state: [u32; N],
    next: usize,
    left: usize,
}

impl core::fmt::Debug for MtRand {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MtRand").field("next", &self.next).field("left", &self.left).finish()
    }
}

fn twist(m: u32, s0: u32, s1: u32) -> u32 {
    m ^ (((s0 & 0x8000_0000) | (s1 & 0x7fff_ffff)) >> 1) ^ ((s1 & 1).wrapping_neg() & 0x9908_b0df)
}

impl MtRand {
    /// Seeds the generator (upstream `MTRand::seed(uint32)` / the `srand` command).
    pub fn new(seed: u32) -> Self {
        let mut rng = Self { state: [0; N], next: 0, left: 0 };
        rng.seed(seed);
        rng
    }

    /// Re-seeds the generator.
    pub fn seed(&mut self, seed: u32) {
        self.state[0] = seed;
        for i in 1..N {
            let prev = self.state[i - 1];
            self.state[i] = 1_812_433_253u32.wrapping_mul(prev ^ (prev >> 30)).wrapping_add(i as u32);
        }
        self.reload();
    }

    fn reload(&mut self) {
        for i in 0..N - M {
            self.state[i] = twist(self.state[i + M], self.state[i], self.state[i + 1]);
        }
        for i in N - M..N - 1 {
            self.state[i] = twist(self.state[i + M - N], self.state[i], self.state[i + 1]);
        }
        self.state[N - 1] = twist(self.state[M - 1], self.state[N - 1], self.state[0]);
        self.left = N;
        self.next = 0;
    }

    /// Integer in `[0, 2^32 - 1]` (upstream `randInt()`).
    pub fn rand_int(&mut self) -> u32 {
        if self.left == 0 {
            self.reload();
        }
        self.left -= 1;
        let mut s1 = self.state[self.next];
        self.next += 1;
        s1 ^= s1 >> 11;
        s1 ^= (s1 << 7) & 0x9d2c_5680;
        s1 ^= (s1 << 15) & 0xefc6_0000;
        s1 ^ (s1 >> 18)
    }

    /// Integer in `[0, n]` (upstream `randInt(n)`: masked rejection sampling).
    pub fn rand_int_max(&mut self, n: u32) -> u32 {
        let mut used = n;
        used |= used >> 1;
        used |= used >> 2;
        used |= used >> 4;
        used |= used >> 8;
        used |= used >> 16;
        loop {
            let i = self.rand_int() & used;
            if i <= n {
                return i;
            }
        }
    }

    /// Real number in `[0, 1]` (upstream `rand()`).
    pub fn rand(&mut self) -> f64 {
        f64::from(self.rand_int()) * (1.0 / 4_294_967_295.0)
    }

    /// Real number in `[0, n]` (upstream `rand(n)`).
    pub fn rand_max(&mut self, n: f64) -> f64 {
        self.rand() * n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference MT19937 outputs for seed 5489.
    #[test]
    fn matches_reference_mt19937() {
        let mut rng = MtRand::new(5489);
        assert_eq!(rng.rand_int(), 3_499_211_612);
        assert_eq!(rng.rand_int(), 581_869_302);
        assert_eq!(rng.rand_int(), 3_890_346_734);
        for _ in 3..9999 {
            rng.rand_int();
        }
        assert_eq!(rng.rand_int(), 4_123_659_995, "10000th output of the reference generator");
    }
}
