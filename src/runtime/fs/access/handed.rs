use crate::runtime::sync::Locked;
use std::collections::HashMap;
use std::os::fd::OwnedFd;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

const UNCLAIMED: Duration = Duration::from_secs(60);

struct Held {
    fd: OwnedFd,
    since: Instant,
}

static HELD: LazyLock<Mutex<HashMap<String, Held>>> = LazyLock::new(Mutex::default);

pub fn hand_fds(fds: Vec<OwnedFd>) -> Vec<String> {
    let mut held = HELD.locked();
    let before = held.len();
    held.retain(|_, kept| kept.since.elapsed() < UNCLAIMED);
    if held.len() < before {
        log::warn!("{} handed descriptor(s) were not claimed within {}s and are closed", before - held.len(), UNCLAIMED.as_secs());
    }
    let now = Instant::now();
    let mut keep = |fd| {
        let token = uuid::Uuid::new_v4().simple().to_string();
        held.insert(token.clone(), Held { fd, since: now });
        token
    };
    fds.into_iter().map(&mut keep).collect()
}

pub(crate) fn claim(tokens: &[String]) -> Vec<Option<OwnedFd>> {
    let mut held = HELD.locked();
    tokens.iter().map(|token| held.remove(token).map(|kept| kept.fd)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor() -> OwnedFd {
        std::fs::File::open("/").unwrap().into()
    }

    #[test]
    fn a_token_is_claimed_once_and_unknown_ones_get_nothing() {
        let tokens = hand_fds(vec![descriptor(), descriptor()]);
        assert_eq!(tokens.len(), 2);
        assert_ne!(tokens[0], tokens[1]);
        let asked = [tokens[1].clone(), "3".to_owned(), tokens[0].clone()];
        let claimed = claim(&asked);
        assert!(claimed[0].is_some() && claimed[1].is_none() && claimed[2].is_some());
        assert!(claim(&tokens).iter().all(Option::is_none));
    }
}
