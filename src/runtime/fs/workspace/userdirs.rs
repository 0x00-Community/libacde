use super::WorkspaceFs;
use crate::runtime::failure::Outcome;
use crate::runtime::fs::path::NodePath;
use std::{
    fs::{self, DirBuilder},
    path::Path,
};

#[cfg(all(windows, not(test)))]
use crate::runtime::fs::path::known_folder;

#[cfg(all(windows, test))]
fn known_folder(name: &str) -> Option<String> {
    std::env::var(format!("ACDE_TEST_KNOWN_{name}")).ok().filter(|dir| !dir.is_empty())
}

impl WorkspaceFs {
    pub fn place_dir(&self, name: &str) -> Option<String> {
        let (key, fallback) = match name {
            "desktop" => ("DESKTOP", "Desktop"),
            "documents" => ("DOCUMENTS", "Documents"),
            "downloads" => ("DOWNLOAD", "Downloads"),
            "music" => ("MUSIC", "Music"),
            "pictures" => ("PICTURES", "Pictures"),
            "videos" => ("VIDEOS", "Videos"),
            _ => return None,
        };
        Some(self.user_dir(key, fallback))
    }

    fn user_dir(&self, name: &str, fallback: &str) -> String {
        let home = self.home.as_str();
        let key = format!("XDG_{name}_DIR");
        let dir = std::env::var(&key).ok().filter(|dir| self.system && !dir.is_empty()).or_else(|| {
            let text = fs::read(home.join(".config").join("user-dirs.dirs")).ok()?;
            let prefix = format!("{key}=\"");
            String::from_utf8_lossy(&text).split(['\n', '\r']).find_map(|line| {
                let rest = line.strip_prefix(prefix.as_str())?;
                let (value, _) = rest.split_once('"')?;
                (!value.is_empty()).then(|| value.to_owned())
            })
        });
        #[cfg(windows)]
        let dir = dir.or_else(|| self.system.then(|| known_folder(name)).flatten());
        let Some(dir) = dir else { return home.join(fallback) };
        let path = match dir.strip_prefix("$HOME") {
            Some(rest) if rest.is_empty() || rest.starts_with('/') || (cfg!(windows) && rest.starts_with('\\')) => {
                format!("{home}{rest}")
            }
            _ => dir,
        };
        let selected = cfg_select! {
            windows => crate::runtime::fs::path::windows_directory(&path).filter(|dir| {
                let same_spelling = crate::runtime::fs::path::windows_directory(home)
                    .is_some_and(|home| dir.eq_ignore_ascii_case(&home));
                let same_location = || fs::canonicalize(home).ok().zip(fs::canonicalize(dir).ok())
                    .is_some_and(|(home, dir)| home == dir);
                !same_spelling && !same_location()
            }),
            _ => path.is_absolute().then(|| Self::directory(&path)).filter(|dir| !Self::same_directory(home, dir)),
        };
        selected.unwrap_or_else(|| home.join(fallback))
    }

    #[cfg(not(windows))]
    fn directory(path: &str) -> String {
        match path.normalize().trim_end_matches('/') {
            "" => "/".into(),
            clean => clean.into(),
        }
    }

    #[cfg(not(windows))]
    fn same_directory(home: &str, dir: &str) -> bool {
        Self::directory(home) == dir || fs::canonicalize(home).ok().zip(fs::canonicalize(dir).ok()).is_some_and(|(home, dir)| home == dir)
    }

    pub fn desktop(&self) -> Outcome<String> {
        let desktop = self.user_dir("DESKTOP", "Desktop");
        if !Path::new(&desktop).exists() {
            DirBuilder::new().recursive(true).create(&desktop)?;
        }
        Ok(desktop)
    }

    pub fn downloads(&self) -> Option<String> {
        Some(self.user_dir("DOWNLOAD", "Downloads")).filter(|dir| Path::new(dir).exists())
    }
}
