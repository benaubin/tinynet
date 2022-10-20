use std::fmt::Debug;

/// A fixed-length bitmap window, useful for eliminating duplicates in a best-effort stream
pub struct Window<const N: usize = 3> {
    map: [usize; N],
    first_index: u64,
}

pub struct Iter<'a, const N: usize> {
    window: &'a Window<N>,
    idx: u64
}

impl<const N: usize> Iterator for Iter<'_, N> {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        let mut adj: usize = self.idx.saturating_sub(self.window.first_index).try_into().ok()?;
        loop {
            if adj >= Window::<N>::LEN {
                return None;
            }
            let word_idx = adj / usize::BITS as usize;
            let word_offset = adj % usize::BITS as usize;
            let mask = 1usize << word_offset;
            let word = self.window.map[word_idx];
            let val = (word & mask) != 0;
            if val {
                let idx = self.window.first_index + adj as u64;
                self.idx = idx + 1;
                return Some(idx);
            }
            adj += 1;
        }
    }
}

impl<const N: usize> Window<N> {
    const LEN: usize = N * usize::BITS as usize;

    /// create a new, empty window
    pub fn new() -> Self {
        Self {
            map: [0; N],
            first_index: 0,
        }
    }

    /// returns true if index can be inserted
    pub fn can_insert(&self, index: u64) -> bool {
        let adjusted_index = match index.checked_sub(self.first_index) {
            Some(offset) => offset,
            None => return false
        };
        let word_idx = adjusted_index as usize / usize::BITS as usize;
        let word_offset = adjusted_index as u32 % usize::BITS;
        let mask = 1usize << word_offset;
        if word_idx >= N { return true }
        self.map[word_idx] & mask == 0
    }

    /// Attemps to insert `index`. 
    /// 
    /// If the index has been inserted before, the insert will return false.
    /// The window may return false when given a lower index than one it has seen before, even if the smaller index has 
    /// not yet been seen.
    pub fn insert(&mut self, index: u64) -> bool {
        let adjusted_index = match index.checked_sub(self.first_index) {
            Some(offset) => offset,
            None => return false
        };
        let mut word_idx = adjusted_index as usize / usize::BITS as usize;
        let word_offset = adjusted_index as u32 % usize::BITS;
        if let Some(gap) = word_idx.checked_sub(N) {
            let keep = (N / 2 + 1).saturating_sub(gap);
            self.map.copy_within(N - keep.., 0);
            self.map[keep..].fill(0);
            word_idx = N / 2 + 1;
            self.first_index += (gap + N / 2) as u64 * usize::BITS as u64;
        }

        let word = &mut self.map[word_idx];
        let mask = 1usize << word_offset;
        let new = (*word & mask) == 0;
        *word |= mask;
        return new;
    }

    pub fn iter<'a>(&'a self) -> Iter<'a, N> {
        Iter {
            window: self,
            idx: self.first_index
        }
    }
}

impl<const N: usize> Debug for Window<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        struct Entries<'a, const N: usize>(&'a Window<N>);
        impl<const N: usize> Debug for Entries<'_, N> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_set().entries(self.0.iter()).finish()
            }
        }
        f.debug_struct("Window").field("con", &Entries(self)).field("first_index", &self.first_index).finish()
    }
}

#[cfg(test)]
mod tests {
    use rand::Rng;

    use super::*;

    #[test]
    fn simple() {
        let mut window = Window::<2>::new();
        for i in 0..128 {
            assert_eq!(window.insert(i), true);
        }
    }

    #[test]
    fn large() {
        let mut window = Window::<10>::new();
        for i in 0..(10 * 64) {
            assert_eq!(window.insert(i), true);
        }
    }

    #[test]
    fn expanding() {
        let mut window = Window::<3>::new();
        for i in 0..(10 * 64) {
            assert_eq!(window.insert(i), true);
        }
    }

    #[test]
    fn expanding_with_skips() {
        let mut window = Window::<5>::new();
        for i in (0..(100 * 64)).step_by(100) {
            assert_eq!(window.insert(i), true);
            assert_eq!(window.insert(i), false);
        }
    }

    #[test]
    fn expanding_with_big_skips() {
        let mut window = Window::<5>::new();
        for i in (0..(1000 * 64)).step_by(1000) {
            assert_eq!(window.insert(i), true, "{i}");
            assert_eq!(window.insert(i), false, "{i}");
            assert_eq!(window.insert(i+1), true, "{i}");
            assert_eq!(window.insert(i+1), false, "{i}");
            assert_eq!(window.insert(i+128), true, "{i}");
            assert_eq!(window.insert(i+128), false, "{i}");
            assert_eq!(window.insert(i), false, "{window:?} {i}");
            assert_eq!(window.insert(i+1), false, "{i}");
        }
    }
    #[test]
    fn expanding_with_random() {
        let mut window = Window::<5>::new();
        let mut r = rand::thread_rng();
        let mut nums = Vec::from_iter(std::iter::repeat_with(|| r.gen_range(0..1_000_000)).take(100_000));
        nums.sort_unstable();
        nums.dedup();

        for chunk in nums.chunks(5000) {
            for n in chunk {
                assert!(window.can_insert(*n), "{window:?} {n}");
                assert!(window.insert(*n), "{window:?} {n}");
                assert!(!window.can_insert(*n), "{window:?} {n}");
                assert!(!window.insert(*n), "{window:?} {n}");
            }
            for n in chunk {
                assert!(!window.can_insert(*n), "{window:?} {n}");
                assert!(!window.insert(*n), "{window:?} {n}");
            }
        }
        for n in nums.iter() {
            assert!(!window.can_insert(*n), "{window:?} {n}");
            assert!(!window.insert(*n), "{window:?} {n}");
        }
    }
}
