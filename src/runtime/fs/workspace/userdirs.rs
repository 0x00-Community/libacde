use super::WorkspaceFs;
use crate::runtime::failure::Outcome;
use crate::runtime::fs::path::NodePath;
use acde_dirs::Place;
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
        Place::named(name).map(|place| self.user_dir(place))
    }

    fn user_dir(&self, place: Place) -> String {
        let home = self.home.as_str();
        let dir = std::env::var(place.variable()).ok().filter(|dir| self.system && !dir.is_empty()).or_else(|| {
            let text = fs::read(acde_dirs::list_file(Path::new(home))).ok()?;
            acde_dirs::listed(&String::from_utf8_lossy(&text), place).map(String::from)
        });
        #[cfg(windows)]
        let dir = dir.or_else(|| self.system.then(|| known_folder(place.key())).flatten());
        let Some(dir) = dir else { return home.join(place.fallback()) };
        let path = acde_dirs::expanded(&dir, home);
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
        selected.unwrap_or_else(|| home.join(place.fallback()))
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
        let desktop = self.user_dir(Place::Desktop);
        if !Path::new(&desktop).exists() {
            DirBuilder::new().recursive(true).create(&desktop)?;
        }
        Ok(desktop)
    }

    pub fn downloads(&self) -> Option<String> {
        Some(self.user_dir(Place::Downloads)).filter(|dir| Path::new(dir).exists())
    }
}
