/// Reproduces the current SuperSpec seeded stream, including its signed middle shift.
/// Changing this algorithm requires a generator-version change and scene review.
#[derive(Clone, Debug)]
pub struct SeededRng {
    state: u32,
}

impl SeededRng {
    pub fn from_u32(seed: u32) -> Self {
        Self {
            state: if seed == 0 { 0x9e37_79b9 } else { seed },
        }
    }

    /// Hashes UTF-16 code units, so non-BMP seeds match the source implementation.
    pub fn from_text(seed: &str) -> Self {
        let mut hash = 2_166_136_261_u32;
        for unit in seed.encode_utf16() {
            hash = (hash ^ u32::from(unit)).wrapping_mul(16_777_619);
        }
        Self::from_u32(hash)
    }

    /// Advances exactly once. The arithmetic shift is intentional, not unsigned xorshift32.
    pub fn next_u32(&mut self) -> u32 {
        let mut value = self.state;
        value ^= value << 13;
        value ^= ((value as i32) >> 17) as u32;
        value ^= value << 5;
        self.state = value;
        value
    }

    pub fn next_f64(&mut self) -> f64 {
        f64::from(self.next_u32()) / 4_294_967_296.0
    }

    pub fn float(&mut self, low: f64, high: f64) -> f64 {
        low + self.next_f64() * (high - low)
    }

    /// Inclusive integer interval; callers supply a nonempty bounded interval.
    pub fn integer(&mut self, low: usize, high: usize) -> usize {
        assert!(low <= high && high < usize::MAX);
        low + (self.next_f64() * (high - low + 1) as f64).floor() as usize
    }

    pub fn chance(&mut self, probability: f64) -> bool {
        self.next_f64() < probability
    }

    pub fn pick<T: Clone>(&mut self, choices: &[T]) -> T {
        assert!(!choices.is_empty());
        choices[self.integer(0, choices.len() - 1)].clone()
    }
}
