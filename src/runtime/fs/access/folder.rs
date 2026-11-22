use crate::runtime::failure::{Failure, Outcome};
#[cfg(not(windows))]
use crate::runtime::fs::path::NodePath;
#[cfg(windows)]
use crate::runtime::fs::path::windows_directory;
use std::path::Path;

pub fn valid_owner(owner: &str) -> bool {
    !owner.is_empty() && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[cfg(windows)]
fn native_folder(path: &str) -> Option<String> {
    let path = windows_directory(path)?;
    let trimmed = path.trim_end_matches(['\\', '/']);
    Some(if Path::new(trimmed).is_absolute() { trimmed.to_owned() } else { path })
}

#[cfg(not(windows))]
fn native_folder(path: &str) -> Option<String> {
    path.is_absolute().then(|| match path.normalize() {
        clean if clean.len() > 1 => clean.trim_end_matches('/').to_owned(),
        clean => clean,
    })
}

pub fn existing(path: &str, what: &str) -> Outcome<String> {
    let path = native_folder(path).ok_or_else(|| Failure::invalid(format!("a {what} folder is an absolute path")))?;
    if !Path::new(&path).is_dir() {
        return Err(Failure::invalid(format!("a {what} folder must be an existing folder")));
    }
    Ok(path)
}
