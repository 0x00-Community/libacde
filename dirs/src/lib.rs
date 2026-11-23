use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    Desktop,
    Documents,
    Downloads,
    Music,
    Pictures,
    Videos,
}

impl Place {
    pub const ALL: [Place; 6] = [Place::Desktop, Place::Documents, Place::Downloads, Place::Music, Place::Pictures, Place::Videos];

    pub fn named(name: &str) -> Option<Place> {
        Self::ALL.into_iter().find(|place| place.name() == name)
    }

    pub fn name(self) -> &'static str {
        match self {
            Place::Desktop => "desktop",
            Place::Documents => "documents",
            Place::Downloads => "downloads",
            Place::Music => "music",
            Place::Pictures => "pictures",
            Place::Videos => "videos",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Place::Desktop => "DESKTOP",
            Place::Documents => "DOCUMENTS",
            Place::Downloads => "DOWNLOAD",
            Place::Music => "MUSIC",
            Place::Pictures => "PICTURES",
            Place::Videos => "VIDEOS",
        }
    }

    pub fn variable(self) -> String {
        format!("XDG_{}_DIR", self.key())
    }

    pub fn fallback(self) -> &'static str {
        match self {
            Place::Desktop => "Desktop",
            Place::Documents => "Documents",
            Place::Downloads => "Downloads",
            Place::Music => "Music",
            Place::Pictures => "Pictures",
            Place::Videos => "Videos",
        }
    }
}

pub fn list_file(home: &Path) -> PathBuf {
    home.join(".config").join("user-dirs.dirs")
}

pub fn listed(text: &str, place: Place) -> Option<&str> {
    let prefix = format!("{}=\"", place.variable());
    text.split(['\n', '\r']).find_map(|line| {
        let (value, _) = line.trim_start().strip_prefix(prefix.as_str())?.split_once('"')?;
        (!value.is_empty()).then_some(value)
    })
}

pub fn expanded(value: &str, home: &str) -> String {
    match value.strip_prefix("$HOME") {
        Some(rest) if rest.is_empty() || rest.starts_with('/') || (cfg!(windows) && rest.starts_with('\\')) => format!("{home}{rest}"),
        _ => value.to_owned(),
    }
}

pub fn user_dir(list: Option<&str>, place: Place, home: &Path) -> PathBuf {
    let given = list.and_then(|text| listed(text, place)).map(|value| PathBuf::from(expanded(value, &home.to_string_lossy())));
    given.filter(|dir| dir.is_absolute() && !same_dir(dir, home)).unwrap_or_else(|| home.join(place.fallback()))
}

fn same_dir(dir: &Path, home: &Path) -> bool {
    dir.components().eq(home.components()) || dir.canonicalize().ok().zip(home.canonicalize().ok()).is_some_and(|(dir, home)| dir == home)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = concat!(
        "# made by xdg-user-dirs-update\n",
        "XDG_DOCUMENTS_DIR=\"$HOME/文档\"\n",
        "XDG_MUSIC_DIR=\"/srv/music\"\n",
        "XDG_DESKTOP_DIR=\"$HOME/\"\n",
        "  XDG_VIDEOS_DIR=\"\"\n",
        "XDG_VIDEOS_DIR=\"$HOMEvideos\"\n",
    );

    #[test]
    fn places_have_one_name_key_and_fallback_each() {
        assert_eq!(Place::ALL.map(|place| Place::named(place.name())), Place::ALL.map(Some));
        assert_eq!(Place::Downloads.variable(), "XDG_DOWNLOAD_DIR");
        assert_eq!(Place::named("removable"), None);
    }

    #[test]
    fn a_listed_value_skips_empty_lines_and_keeps_the_rest_raw() {
        assert_eq!(listed(LIST, Place::Documents), Some("$HOME/文档"));
        assert_eq!(listed(LIST, Place::Videos), Some("$HOMEvideos"), "an empty value counts as not listed");
        assert_eq!(listed(LIST, Place::Pictures), None);
        assert_eq!(expanded("$HOME/文档", "/home/u"), "/home/u/文档");
        assert_eq!(expanded("$HOME", "/home/u"), "/home/u");
        assert_eq!(expanded("$HOMEvideos", "/home/u"), "$HOMEvideos", "only $HOME on its own is the home folder");
    }

    #[test]
    fn a_user_dir_is_the_listed_one_or_the_fallback_never_home() {
        let home = Path::new("/home/u");
        assert_eq!(user_dir(Some(LIST), Place::Documents, home), PathBuf::from("/home/u/文档"));
        assert_eq!(user_dir(Some(LIST), Place::Music, home), PathBuf::from("/srv/music"));
        assert_eq!(user_dir(Some(LIST), Place::Pictures, home), PathBuf::from("/home/u/Pictures"));
        assert_eq!(user_dir(Some(LIST), Place::Desktop, home), PathBuf::from("/home/u/Desktop"), "the home folder itself is never a place");
        assert_eq!(user_dir(Some(LIST), Place::Videos, home), PathBuf::from("/home/u/Videos"), "a relative value falls back");
        assert_eq!(user_dir(None, Place::Downloads, home), PathBuf::from("/home/u/Downloads"));
    }
}
