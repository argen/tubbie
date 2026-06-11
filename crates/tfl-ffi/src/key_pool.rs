use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

pub struct KeyPool {
    keys: Vec<String>,
    cursor: AtomicUsize,
}

impl KeyPool {
    /// Validates each key as 32 lowercase-hex. Returns `None` if no valid
    /// keys remain — callers must not initialise the pool with zero slots
    /// (which would cause divide-by-zero in `pick()`).
    pub fn new(keys: Vec<String>) -> Option<Self> {
        let valid: Vec<String> = keys
            .into_iter()
            .filter(|k| k.len() == 32 && k.chars().all(|c| c.is_ascii_hexdigit()))
            .collect();
        if valid.is_empty() {
            return None;
        }
        Some(Self {
            keys: valid,
            cursor: AtomicUsize::new(0),
        })
    }

    /// Returns `(slot_index, key_str)`. Guaranteed non-panicking: `keys`
    /// is non-empty by construction (`new` returns `None` otherwise) and
    /// `Relaxed` ordering is correct — the cursor's only job is load
    /// distribution; no data publication or observation depends on it.
    pub fn pick(&self) -> (usize, &str) {
        let idx = self.cursor.fetch_add(1, Relaxed) % self.keys.len();
        (idx, &self.keys[idx])
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_key(prefix: u8) -> String {
        format!("{:0<32}", format!("{:x}", prefix))
    }

    fn valid_keys(n: usize) -> Vec<String> {
        (0..n).map(|i| make_key(i as u8)).collect()
    }

    #[test]
    fn pool_new_with_empty_keys_returns_none() {
        assert!(KeyPool::new(vec![]).is_none());
    }

    #[test]
    fn pool_new_with_all_invalid_keys_returns_none() {
        let bad = vec![
            "tooshort".to_string(),
            "UPPERCASE_32_CHARS______________".to_string(),
            "contains-hyphens-and-stuff-32ch".to_string(),
            "z".repeat(32), // 'z' is not hex
        ];
        assert!(KeyPool::new(bad).is_none());
    }

    #[test]
    fn pool_new_with_some_invalid_keys_skips_bad_entries() {
        let good = make_key(1);
        let keys = vec!["bad".to_string(), good.clone(), "alsoBad!".to_string()];
        let pool = KeyPool::new(keys).expect("should have one valid key");
        assert_eq!(pool.len(), 1);
        let (_, key) = pool.pick();
        assert_eq!(key, good);
    }

    #[test]
    fn pool_pick_rotates_across_all_slots() {
        let n = 4;
        let pool = KeyPool::new(valid_keys(n)).unwrap();
        let mut seen = vec![false; n];
        for _ in 0..n {
            let (idx, _) = pool.pick();
            seen[idx] = true;
        }
        assert!(
            seen.iter().all(|&v| v),
            "every slot must be picked once in N picks"
        );
    }

    #[test]
    fn pool_cursor_wraps_after_last_slot() {
        let n = 3;
        let pool = KeyPool::new(valid_keys(n)).unwrap();
        // Exhaust one full cycle.
        for _ in 0..n {
            pool.pick();
        }
        // Next pick wraps back to slot 0.
        let (idx, _) = pool.pick();
        assert_eq!(idx, 0, "cursor must wrap to slot 0 after N picks");
    }

    #[test]
    fn pool_pick_is_concurrent_safe() {
        use std::sync::Arc;
        use std::thread;

        let n = 6usize;
        let pool = Arc::new(KeyPool::new(valid_keys(n)).unwrap());
        let picks_per_thread = 1000usize;

        let handles: Vec<_> = (0..n)
            .map(|_| {
                let p = Arc::clone(&pool);
                thread::spawn(move || {
                    let mut counts = vec![0usize; n];
                    for _ in 0..picks_per_thread {
                        let (idx, _) = p.pick();
                        counts[idx] += 1;
                    }
                    counts
                })
            })
            .collect();

        let mut totals = vec![0usize; n];
        for h in handles {
            let counts = h.join().expect("thread panicked");
            for (i, c) in counts.iter().enumerate() {
                totals[i] += c;
            }
        }

        let total_picks = n * picks_per_thread;
        let sum: usize = totals.iter().sum();
        assert_eq!(sum, total_picks, "total pick count must be exact");

        // Distribution: each slot should be within 10% of ideal.
        let ideal = total_picks / n;
        for (i, &count) in totals.iter().enumerate() {
            let diff = (count as isize - ideal as isize).unsigned_abs();
            assert!(
                diff * 10 <= ideal,
                "slot {i} count {count} is too far from ideal {ideal}"
            );
        }
    }
}
