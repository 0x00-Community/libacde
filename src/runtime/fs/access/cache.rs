use crate::runtime::failure::Outcome;
use crate::runtime::fs::workspace::FinderRoot;
use crate::runtime::sync::Locked;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MAX_AGE: Duration = Duration::from_secs(2);

struct Kept {
    grants: u64,
    at: Instant,
    roots: Vec<FinderRoot>,
}

#[derive(Clone, Default)]
pub(in crate::runtime::fs) struct RootCache(Arc<Mutex<Option<Kept>>>);

impl RootCache {
    pub(in crate::runtime::fs) fn get(&self, grants: u64, build: impl FnOnce() -> Outcome<Vec<FinderRoot>>) -> Outcome<Vec<FinderRoot>> {
        let kept = self.0.locked().as_ref().filter(|kept| kept.grants == grants && kept.at.elapsed() < MAX_AGE).map(|kept| kept.roots.clone());
        match kept {
            Some(roots) => Ok(roots),
            None => Ok(self.keep(grants, build()?)),
        }
    }

    pub(in crate::runtime::fs) fn keep(&self, grants: u64, roots: Vec<FinderRoot>) -> Vec<FinderRoot> {
        *self.0.locked() = Some(Kept { grants, at: Instant::now(), roots: roots.clone() });
        roots
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::fs::workspace::RootSource;
    use std::cell::Cell;

    #[test]
    fn roots_are_built_once_until_the_grants_change() {
        let cache = RootCache::default();
        let builds = Cell::new(0);
        let build = || -> Outcome<Vec<FinderRoot>> {
            builds.set(builds.get() + 1);
            Ok(vec![FinderRoot::new("/a".into(), "a", RootSource::Default)])
        };
        for _ in 0..1000 {
            assert_eq!(cache.get(0, build).unwrap().len(), 1);
        }
        assert_eq!(builds.get(), 1);
        cache.get(1, build).unwrap();
        assert_eq!(builds.get(), 2);
        cache.keep(1, Vec::new());
        assert!(cache.get(1, build).unwrap().is_empty());
        assert_eq!(builds.get(), 2);
    }
}
