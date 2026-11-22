use super::{MOST_LENGTH, px};

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Name(u64);

impl Name {
    pub fn of(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() || text == "none" {
            return None;
        }
        Some(Self(text.bytes().fold(FNV_OFFSET, |h, b| (h ^ u64::from(b)).wrapping_mul(FNV_PRIME))))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Membership {
    pub container: Option<u64>,
    pub spacing: f32,
    pub union: Option<Name>,
    pub id: Option<Name>,
}

impl Membership {
    pub fn from_properties(container: Option<(u64, &str)>, property: impl Fn(&str) -> Option<String>, warn: impl Fn(String)) -> Option<Self> {
        let spacing = container.and_then(|(_, text)| match text.trim() {
            "none" | "" => None,
            given => {
                let spacing = px(given);
                if spacing.is_none() {
                    warn(format!("--acde-glass-merge: {given:?} is left out (a length)"));
                }
                spacing
            }
        });
        let container = container.filter(|_| spacing.is_some()).map(|(id, _)| id);
        let union = property("acde-glass-union").and_then(|t| Name::of(&t));
        let id = property("acde-glass-id").and_then(|t| Name::of(&t));
        let spacing = spacing.unwrap_or(0.0).clamp(0.0, MOST_LENGTH);
        (container.is_some() || union.is_some()).then_some(Self { container, spacing, union, id })
    }

    pub fn scaled(self, factor: f64) -> Self {
        Self { spacing: self.spacing * factor as f32, ..self }
    }

    pub fn joins(&self, other: &Self) -> bool {
        (self.container.is_some() && self.container == other.container) || (self.union.is_some() && self.union == other.union)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn properties<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| pairs.iter().find(|(k, _)| *k == name).map(|(_, v)| (*v).to_owned())
    }

    #[test]
    fn a_container_spacing_makes_a_member_and_names_are_kept() {
        let given = [("acde-glass-id", " dock-item "), ("acde-glass-union", "none")];
        let member = Membership::from_properties(Some((7, "12px")), properties(&given), |w| panic!("{w}")).unwrap();
        assert_eq!(member, Membership { container: Some(7), spacing: 12.0, union: None, id: Name::of("dock-item") });
        assert_eq!(member.scaled(2.0).spacing, 24.0);
    }

    #[test]
    fn no_membership_without_a_container_or_a_union_and_a_bad_spacing_warns() {
        assert_eq!(Membership::from_properties(None, properties(&[("acde-glass-id", "a")]), |w| panic!("{w}")), None);
        assert_eq!(Membership::from_properties(Some((1, "none")), properties(&[]), |w| panic!("{w}")), None);
        let warned = std::cell::Cell::new(0);
        assert_eq!(Membership::from_properties(Some((1, "2em")), properties(&[]), |_| warned.set(warned.get() + 1)), None);
        assert_eq!(warned.get(), 1);
        let union = Membership::from_properties(None, properties(&[("acde-glass-union", "pair")]), |w| panic!("{w}")).unwrap();
        assert_eq!((union.container, union.union), (None, Name::of("pair")));
    }

    #[test]
    fn members_join_by_container_or_by_union() {
        let member = |container: Option<u64>, union: Option<&str>| Membership { container, spacing: 8.0, union: union.and_then(Name::of), id: None };
        assert!(member(Some(1), None).joins(&member(Some(1), None)));
        assert!(!member(Some(1), None).joins(&member(Some(2), None)));
        assert!(member(Some(1), Some("u")).joins(&member(None, Some("u"))));
        assert!(!member(None, None).joins(&member(None, None)));
        assert_ne!(Name::of("a"), Name::of("b"));
    }
}
