use core::sync::atomic::{AtomicU32, Ordering};

static PREFERENCE: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Preference {
    pub level: u32,
    pub reduced_transparency: bool,
    pub dark: bool,
}

impl Preference {
    fn to_bits(self) -> u32 {
        self.level.min(u32::MAX >> 2) << 2 | u32::from(self.dark) << 1 | u32::from(self.reduced_transparency)
    }

    fn from_bits(bits: u32) -> Self {
        Self { level: bits >> 2, reduced_transparency: bits & 1 == 1, dark: bits & 2 == 2 }
    }
}

pub fn set_preference(preference: Preference) -> bool {
    PREFERENCE.swap(preference.to_bits(), Ordering::AcqRel) != preference.to_bits()
}

pub fn preference() -> Preference {
    Preference::from_bits(PREFERENCE.load(Ordering::Acquire))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_preference_round_trips_and_says_when_it_changed() {
        let given = Preference { level: 2, reduced_transparency: true, dark: true };
        assert_eq!(Preference::from_bits(given.to_bits()), given);
        assert_eq!(Preference::from_bits(Preference::default().to_bits()), Preference::default());
    }
}
