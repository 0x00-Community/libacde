use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub const MAX_SAMPLES: usize = 1 << 28;

#[derive(Default)]
pub struct Budget {
    samples: AtomicUsize,
    pub cancelled: AtomicBool,
}

impl Budget {
    pub fn used(&self) -> usize {
        self.samples.load(Ordering::Acquire)
    }
}

pub struct Ticket {
    budget: Arc<Budget>,
    samples: usize,
}

impl Ticket {
    pub fn take(budget: &Arc<Budget>, samples: usize) -> Option<Ticket> {
        budget.samples.try_update(Ordering::AcqRel, Ordering::Acquire, |used| used.checked_add(samples).filter(|total| *total <= MAX_SAMPLES)).ok()?;
        Some(Ticket { budget: budget.clone(), samples })
    }

    pub fn budget(&self) -> &Arc<Budget> {
        &self.budget
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        self.budget.samples.fetch_sub(self.samples, Ordering::AcqRel);
    }
}

pub fn zeroed<T: Copy>(len: usize, value: T) -> Option<Vec<T>> {
    let mut v = Vec::new();
    v.try_reserve_exact(len).ok()?;
    v.resize(len, value);
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tickets_share_one_cap_and_give_back_on_drop() {
        let budget = Arc::new(Budget::default());
        let first = Ticket::take(&budget, MAX_SAMPLES - 10).unwrap();
        assert!(Ticket::take(&budget, 11).is_none(), "over the cap while the first is held");
        let small = Ticket::take(&budget, 10).unwrap();
        assert_eq!(budget.used(), MAX_SAMPLES);
        drop((first, small));
        assert_eq!(budget.used(), 0);
    }
}
