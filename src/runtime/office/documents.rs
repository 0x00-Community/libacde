use super::blank::BlankKind;
use crate::runtime::failure::{Errno, Failure, Outcome};
use crate::runtime::fs::Shared as Finder;
use crate::runtime::fs::path::NodePath;
use crate::runtime::fs::workspace::{Create, EntryKind, FinderEntry, FinderRoot, ModifiedMs, WorkspaceFs, WriteOptions};
use serde::Serialize;
use serde_json::{Value, json};
use std::sync::Arc;

pub const MAX_DOCUMENT_BYTES: u64 = 64 * 1024 * 1024;
const EDITABLE: [(&str, &str); 3] = [("docx", "word"), ("xlsx", "cell"), ("pptx", "slide")];
const VIEWABLE: [(&str, &str); 8] =
    [("doc", "word"), ("odt", "word"), ("rtf", "word"), ("xls", "cell"), ("ods", "cell"), ("ppt", "slide"), ("odp", "slide"), ("pdf", "pdf")];

pub struct OfficeFile {
    pub root: FinderRoot,
    pub path: String,
    pub name: String,
    pub ext: String,
    pub document_type: String,
    pub size: u64,
    pub modified_ms: i64,
    pub editable: bool,
    pub read_only_reason: Option<String>,
}

impl OfficeFile {
    pub fn view(&self) -> Value {
        json!({"root": self.root.id, "rootLabel": self.root.label, "path": self.path, "name": self.name, "ext": self.ext,
            "documentType": self.document_type, "size": self.size, "modifiedMs": self.modified_ms, "editable": self.editable,
            "readOnlyReason": self.read_only_reason})
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficeEntry {
    name: String,
    kind: String,
    size: u64,
    modified_ms: i64,
    ext: String,
}

pub struct SaveResult {
    pub path: String,
    pub modified_ms: i64,
    pub conflict: bool,
}

pub struct OfficeFiles(pub Arc<Finder>);

impl OfficeFiles {
    fn stamp() -> String {
        jiff::Timestamp::now().to_zoned(jiff::tz::TimeZone::system()).strftime("%m-%d %H-%M").to_string()
    }

    pub fn extension(path: &str) -> String {
        path.extname().trim_start_matches('.').to_lowercase()
    }

    fn document_type(path: &str) -> Option<String> {
        let ext = Self::extension(path);
        EDITABLE.iter().chain(VIEWABLE.iter()).find(|(e, _)| *e == ext).map(|(_, t)| (*t).to_owned())
    }

    pub fn handed(dir: &str) -> FinderRoot {
        FinderRoot { id: String::new(), shown: None, label: String::new(), ..WorkspaceFs::folder(dir) }
    }

    pub fn file(&self, root_path: &str, relative: &str) -> Outcome<OfficeFile> {
        Self::file_in(WorkspaceFs::folder(root_path), relative)
    }

    pub fn read_only(mut file: OfficeFile) -> OfficeFile {
        if file.editable {
            file.editable = false;
            file.read_only_reason = Some("交来的文件只能查看。".into());
        }
        file
    }

    pub fn file_in(root: FinderRoot, relative: &str) -> Outcome<OfficeFile> {
        let (path, _, info) = WorkspaceFs::opened(&root, relative)?;
        let name = path.basename().to_owned();
        let ext = Self::extension(&name);
        let Some(document_type) = Self::document_type(&name) else {
            let supported: Vec<&str> = EDITABLE.iter().chain(VIEWABLE.iter()).map(|(e, _)| *e).collect();
            let shown = if ext.is_empty() { "?" } else { &ext };
            return Err(Failure::invalid(format!(".{shown} files do not open in Office; supported: {}", supported.join(", "))));
        };
        if info.len() > MAX_DOCUMENT_BYTES {
            return Err(Failure::http(413, "file exceeds 64 MiB"));
        }
        let reason = (!EDITABLE.iter().any(|(e, _)| *e == ext)).then(|| format!(".{ext} 只能查看；要编辑请另存为 docx、xlsx 或 pptx。"));
        Ok(OfficeFile {
            path,
            name,
            ext,
            document_type,
            size: info.len(),
            modified_ms: info.modified_ms(),
            editable: reason.is_none(),
            read_only_reason: reason,
            root,
        })
    }

    pub fn read(file: &OfficeFile) -> Outcome<Vec<u8>> {
        Ok(WorkspaceFs::raw_in(&file.root, &file.path)?.1)
    }

    pub fn list_in(&self, root: FinderRoot, relative: &str) -> Outcome<(String, Vec<OfficeEntry>, bool)> {
        let listing = self.0.fs.list_in(root, relative, false)?;
        let entries = listing
            .entries
            .into_iter()
            .filter_map(|entry| match entry.kind {
                EntryKind::Directory => {
                    Some(OfficeEntry { name: entry.name, kind: "directory".into(), size: 0, modified_ms: entry.modified_ms, ext: String::new() })
                }
                EntryKind::File if Self::document_type(&entry.name).is_some() => {
                    let ext = Self::extension(&entry.name);
                    Some(OfficeEntry { name: entry.name, kind: "document".into(), size: entry.size, modified_ms: entry.modified_ms, ext })
                }
                _ => None,
            })
            .collect();
        Ok((listing.path, entries, listing.truncated))
    }

    fn write_free(root: &FinderRoot, path: &str, data: &[u8]) -> Outcome<(String, FinderEntry)> {
        let fresh = WriteOptions { create: Create::New, ..WriteOptions::default() };
        for index in 1..1000 {
            let candidate = WorkspaceFs::numbered(path, index);
            match WorkspaceFs::write_file_in(root, &candidate, data, &fresh) {
                Ok(entry) => return Ok((candidate, entry)),
                Err(failure) if failure.errno == Errno::Exist => continue,
                Err(failure) => return Err(failure),
            }
        }
        Err(Failure::http(409, "too many files with this name"))
    }

    pub fn create(root: &FinderRoot, directory: &str, kind: BlankKind, name: Option<&str>) -> Outcome<String> {
        let ext = kind.extension();
        let base = crate::runtime::js::JsText::js_trim(name.unwrap_or(kind.default_name())).to_owned();
        let base = match base.len().checked_sub(ext.len() + 1) {
            Some(cut) if base.is_char_boundary(cut) && base[cut..].eq_ignore_ascii_case(&format!(".{ext}")) => base[..cut].to_owned(),
            _ => base,
        };
        if base.is_empty() || base.contains(['/', '\\', '\0']) || base.starts_with('.') {
            return Err(Failure::invalid("invalid file name"));
        }
        let folder = WorkspaceFs::inner(directory)?;
        let name = format!("{base}.{ext}");
        let path = if folder.is_empty() { name } else { format!("{folder}/{name}") };
        Ok(Self::write_free(root, &path, &kind.create())?.0)
    }

    pub fn save(&self, root_path: &str, relative: &str, data: &[u8], expected_ms: i64) -> Outcome<SaveResult> {
        let root = WorkspaceFs::folder(root_path);
        let relative = WorkspaceFs::inner(relative)?;
        let unchanged = WriteOptions { create: Create::Never, if_modified_ms: Some(expected_ms as f64), ..WriteOptions::default() };
        match WorkspaceFs::write_file_in(&root, &relative, data, &unchanged) {
            Ok(entry) => return Ok(SaveResult { path: relative, modified_ms: entry.modified_ms, conflict: false }),
            Err(failure) if failure.errno == Errno::Conflict => {}
            Err(failure) => return Err(failure),
        }
        let ext = relative.extname();
        let copy = format!("{} (冲突副本 {}){ext}", &relative[..relative.len() - ext.len()], Self::stamp());
        let (path, entry) = Self::write_free(&root, &copy, data)?;
        Ok(SaveResult { path, modified_ms: entry.modified_ms, conflict: true })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::fs::{Files, Workspace};
    use std::fs;

    #[test]
    fn level_1_roots_open_and_create_documents_and_handed_files_hide_their_path() {
        let home = std::env::temp_dir().join(format!("acde-office-roots-{}", std::process::id()));
        _ = fs::remove_dir_all(&home);
        fs::create_dir_all(home.join("Documents")).unwrap();
        let text = home.to_string_lossy().into_owned();
        let app = Workspace::at(&text).files(Files::new(&["documents".to_owned()], false), "writer");
        let roots = app.finder_roots().unwrap();
        assert_eq!(roots.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["private", "documents"]);
        assert!(!serde_json::to_string(&roots).unwrap().contains(&text), "roots carry no paths");
        for root in &roots {
            let path = OfficeFiles::create(root, "", BlankKind::Docx, Some("report")).unwrap();
            assert_eq!(path, "report.docx");
            assert_eq!(OfficeFiles::create(root, "", BlankKind::Docx, Some("report")).unwrap(), "report 2.docx");
            let file = OfficeFiles::file_in(root.clone(), &path).unwrap();
            assert!(file.editable);
            assert_eq!(file.view()["root"], root.id.as_str());
            assert!(!file.view().to_string().contains(&text), "{}", file.view());
        }
        assert!(OfficeFiles::create(&roots[1], "../..", BlankKind::Docx, None).is_err());
        let documents = home.join("Documents").to_string_lossy().into_owned();
        let handed = OfficeFiles::file_in(OfficeFiles::handed(&documents), "report.docx").unwrap();
        assert_eq!(handed.view()["root"], "");
        assert!(!handed.view().to_string().contains(&text), "{}", handed.view());
        assert!(handed.editable);
        let read_only = OfficeFiles::read_only(handed);
        assert!(!read_only.editable && read_only.read_only_reason.is_some());
        assert!(OfficeFiles::file_in(OfficeFiles::handed(&documents), "../Documents/report.docx").is_err());
        _ = fs::remove_dir_all(&home);
    }
}
