use crate::runtime::failure::{Failure, Outcome};
use crate::runtime::fs::workspace::FinderRoot;
use std::path::Path;

pub(in crate::runtime::fs) fn real_inside(allowed: &[FinderRoot], path: &str) -> Outcome<String> {
    let real = std::fs::canonicalize(path)?;
    let inside = allowed.iter().filter_map(FinderRoot::on_disk).any(|top| std::fs::canonicalize(top).is_ok_and(|top| real.starts_with(&top)));
    if !inside {
        return Err(Failure::http(403, "not inside a folder the caller may open"));
    }
    plain(&real).ok_or_else(|| Failure::invalid("the path is not valid Unicode"))
}

#[cfg(windows)]
fn plain(real: &Path) -> Option<String> {
    let text = real.to_str()?;
    Some(match text.strip_prefix(r"\\?\") {
        Some(rest) => match rest.strip_prefix(r"UNC\") {
            Some(share) => format!(r"\\{share}"),
            None => rest.to_owned(),
        },
        None => text.to_owned(),
    })
}

#[cfg(not(windows))]
fn plain(real: &Path) -> Option<String> {
    real.to_str().map(str::to_owned)
}
