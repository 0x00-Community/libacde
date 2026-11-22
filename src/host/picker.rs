use std::cell::RefCell;

use crate::model::{FileFilter, OpenRequest, SaveRequest};
use crate::runtime::{Dropped, EntryKind, FsError, LentFile, LentRoot, Opened, PickedFolders, Range, Workspace};

use crate::host::{bind, offload};

#[derive(Clone)]
pub struct Picker {
    folders: PickedFolders,
}

enum Reply {
    Entries(Vec<(String, EntryKind)>),
    Kind(Option<EntryKind>),
    Removed(bool),
    Read(Option<(Vec<u8>, i64)>),
    Range(Option<Range>),
    Opened(Opened),
    Dropped(Vec<Dropped>),
    Done,
}

type Answer = Result<Reply, FsError>;

impl Picker {
    pub fn new(app: Option<Workspace>) -> Self {
        Self { folders: PickedFolders::lent_from(app) }
    }
}

const DROP_KEPT: std::time::Duration = std::time::Duration::from_secs(5);

thread_local! {
    static PICKER: RefCell<Option<Picker>> = const { RefCell::new(None) };
    static DROPPED: RefCell<Option<(Vec<String>, std::time::Instant)>> = const { RefCell::new(None) };
}

pub fn note_drop(files: &[String]) {
    DROPPED.with(|dropped| *dropped.borrow_mut() = Some((files.to_vec(), std::time::Instant::now())));
}

pub fn adopt(path: &str) -> Result<String, String> {
    let picker = PICKER.with(|p| p.borrow().clone()).ok_or("this web has no file handles")?;
    picker.folders.add(path).map_err(|e| e.message)
}

pub fn install(scope: &mut v8::PinScope, natives: v8::Local<v8::Object>, picker: Picker) {
    PICKER.with(|p| *p.borrow_mut() = Some(picker));
    let object = v8::Object::new(scope);
    crate::methods!(scope, object,
        "entries" => entries,
        "entry" => entry,
        "remove" => remove,
        "read" => read,
        "write" => write,
        "readRange" => read_range,
        "openWrite" => open_write,
        "writeChunk" => write_chunk,
        "truncateWrite" => truncate_write,
        "commitWrite" => commit_write,
        "abortWrite" => abort_write,
        "dropped" => dropped,
    );
    bind::set(scope, natives, "picker", object.into());
}

fn run(scope: &mut v8::PinScope, rv: &mut v8::ReturnValue, work: impl FnOnce(Picker) -> Answer + Send + 'static) {
    let Some(picker) = PICKER.with(|p| p.borrow().clone()) else {
        return bind::throw_error(scope, "picker: not given to this web");
    };
    offload::run(scope, rv, "acde-picker", move || work(picker), settle);
}

fn kind(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> Option<EntryKind> {
    match value.is_string().then(|| value.to_rust_string_lossy(scope)).as_deref() {
        Some("file") => Some(EntryKind::File),
        Some("directory") => Some(EntryKind::Directory),
        _ => None,
    }
}

fn entries(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let dir = bind::arg_string(scope, &args, 0);
    run(scope, &mut rv, move |picker| Ok(Reply::Entries(picker.folders.entries(&dir)?)));
}

fn entry(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let (dir, name) = (bind::arg_string(scope, &args, 0), bind::arg_string(scope, &args, 1));
    let create = kind(scope, args.get(2));
    run(scope, &mut rv, move |picker| Ok(Reply::Kind(picker.folders.child(&dir, &name, create)?)));
}

fn remove(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let (dir, name) = (bind::arg_string(scope, &args, 0), bind::arg_string(scope, &args, 1));
    let recursive = args.get(2).boolean_value(scope);
    run(scope, &mut rv, move |picker| Ok(Reply::Removed(picker.folders.remove(&dir, &name, recursive)?)));
}

fn read(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let path = bind::arg_string(scope, &args, 0);
    run(scope, &mut rv, move |picker| Ok(Reply::Read(picker.folders.read(&path)?.map(|contents| (contents.data, contents.modified_ms)))));
}

fn write(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let path = bind::arg_string(scope, &args, 0);
    let Some(data) = bind::with_bytes(args.get(1), <[u8]>::to_vec) else {
        return bind::throw_type_error(scope, "picker.write: the data is bytes");
    };
    run(scope, &mut rv, move |picker| {
        picker.folders.write(&path, &data)?;
        Ok(Reply::Done)
    });
}

fn count(scope: &mut v8::PinScope, args: &v8::FunctionCallbackArguments, i: i32) -> Option<u64> {
    let n = args.get(i).number_value(scope)?;
    (n.is_finite() && n >= 0.0 && n.fract() == 0.0 && n <= 9_007_199_254_740_991.0).then_some(n as u64)
}

fn read_range(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let path = bind::arg_string(scope, &args, 0);
    let (offset, length) = (count(scope, &args, 1), count(scope, &args, 2));
    let version = args.get(3).is_string().then(|| bind::arg_string(scope, &args, 3));
    run(scope, &mut rv, move |picker| Ok(Reply::Range(picker.folders.read_range(&path, offset, length, version)?)));
}

fn open_write(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let path = bind::arg_string(scope, &args, 0);
    let keep = bind::arg_bool(scope, &args, 1);
    run(scope, &mut rv, move |picker| Ok(Reply::Opened(picker.folders.open_write(&path, keep)?)));
}

fn write_chunk(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let id = bind::arg_string(scope, &args, 0);
    let offset = count(scope, &args, 1);
    let Some(data) = bind::with_bytes(args.get(2), <[u8]>::to_vec) else {
        return bind::throw_type_error(scope, "picker.writeChunk: the data is bytes");
    };
    run(scope, &mut rv, move |picker| {
        picker.folders.write_chunk(&id, offset, &data)?;
        Ok(Reply::Done)
    });
}

fn truncate_write(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let id = bind::arg_string(scope, &args, 0);
    let size = count(scope, &args, 1);
    run(scope, &mut rv, move |picker| {
        picker.folders.truncate_write(&id, size)?;
        Ok(Reply::Done)
    });
}

fn commit_write(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let id = bind::arg_string(scope, &args, 0);
    run(scope, &mut rv, move |picker| {
        picker.folders.commit_write(&id)?;
        Ok(Reply::Done)
    });
}

fn dropped(scope: &mut v8::PinScope, _: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let taken = DROPPED.with(|dropped| dropped.borrow_mut().take());
    let files = taken.filter(|(_, at)| at.elapsed() < DROP_KEPT).map(|(files, _)| files).unwrap_or_default();
    run(scope, &mut rv, move |picker| Ok(Reply::Dropped(picker.folders.lend_dropped(&files))));
}

fn abort_write(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let id = bind::arg_string(scope, &args, 0);
    run(scope, &mut rv, move |picker| {
        picker.folders.abort_write(&id)?;
        Ok(Reply::Done)
    });
}

fn settle(scope: &mut v8::PinScope, resolver: v8::Local<v8::PromiseResolver>, answer: Answer) {
    let value: v8::Local<v8::Value> = match answer {
        Ok(Reply::Kind(None) | Reply::Read(None) | Reply::Range(None)) => v8::null(scope).into(),
        Ok(Reply::Range(Some(range))) => crate::js_object!(scope, {
            "bytes" => range.data, "offset" => range.offset, "size" => range.size, "lastModified" => range.modified_ms, "version" => range.version
        })
        .into(),
        Ok(Reply::Opened(opened)) => crate::js_object!(scope, {"id" => opened.id, "size" => opened.size}).into(),
        Ok(Reply::Kind(Some(kind))) => bind::string(scope, kind.as_str()).into(),
        Ok(Reply::Removed(removed)) => v8::Boolean::new(scope, removed).into(),
        Ok(Reply::Done) => v8::undefined(scope).into(),
        Ok(Reply::Dropped(dropped)) => {
            let list = v8::Array::new(scope, 0);
            for Dropped { name, label, kind } in dropped {
                let entry = crate::js_object!(scope, {"name" => name, "label" => label, "kind" => kind.as_str(), "writable" => false});
                list.set_index(scope, list.length(), entry.into());
            }
            list.into()
        }
        Ok(Reply::Entries(children)) => {
            let list = v8::Array::new(scope, 0);
            for (name, kind) in children.into_iter().filter(|(_, kind)| matches!(kind, EntryKind::File | EntryKind::Directory)) {
                let entry = crate::js_object!(scope, {"name" => name, "kind" => kind.as_str()});
                list.set_index(scope, list.length(), entry.into());
            }
            list.into()
        }
        Ok(Reply::Read(Some((data, modified_ms)))) => crate::js_object!(scope, {"bytes" => data, "lastModified" => modified_ms}).into(),
        Err(FsError { status, code, message }) => {
            let error = offload::failure(scope, &message, Some(i32::from(status)), code);
            resolver.reject(scope, error);
            return;
        }
    };
    resolver.resolve(scope, value);
}

type Chosen = Result<Option<(String, LentRoot)>, String>;

pub fn pick_folder(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let owner = bind::arg_string(scope, &args, 0);
    let title = bind::arg_string(scope, &args, 1);
    let writable = bind::arg_bool(scope, &args, 2);
    let platform = crate::host::rt::current().platform();
    let choose = move || -> Chosen {
        let Some(path) = platform.choose_folder(&title)? else { return Ok(None) };
        let label = std::path::Path::new(&path).file_name().map_or_else(|| path.clone(), |name| name.to_string_lossy().into_owned());
        let root = LentRoot::open(label, writable, &path).map_err(|error| format!("{path}: {error}"))?;
        Ok(Some((owner, root)))
    };
    offload::run(scope, &mut rv, "acde-picker", choose, lend_chosen);
}

fn lend_chosen(scope: &mut v8::PinScope, resolver: v8::Local<v8::PromiseResolver>, chosen: Chosen) {
    let lent = chosen.and_then(|chosen| {
        let Some((owner, root)) = chosen else { return Ok(None) };
        let label = root.label.clone();
        crate::host::web::lend(&owner, root).map(|name| Some((name, label))).map_err(|error| error.message)
    });
    let picked = lent.map(|lent| match lent {
        Some((name, label)) => crate::js_object!(scope, {"name" => name, "label" => label}).into(),
        None => v8::null(scope).into(),
    });
    answer(scope, resolver, picked);
}

struct Files {
    owner: String,
    files: Vec<LentFile>,
    save: bool,
}

type ChosenFiles = Result<Option<Files>, String>;

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct Filter {
    name: String,
    globs: Vec<String>,
    mimes: Vec<String>,
}

fn filters(given: Option<serde_json::Value>) -> Vec<FileFilter> {
    let given: Vec<Filter> = given.and_then(|value| serde_json::from_value(value).ok()).unwrap_or_default();
    given.into_iter().map(|Filter { name, globs, mimes }| FileFilter { name, globs, mimes }).collect()
}

fn opened(paths: Vec<String>, writable: bool) -> Result<Vec<LentFile>, String> {
    paths.iter().map(|path| LentFile::open(path, writable).map_err(|error| format!("{path}: {error}"))).collect()
}

pub fn pick_files(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let mut given = bind::Args { scope: &mut *scope, args: &args };
    let (owner, title, multiple, filters) = (given.string(0), given.string(1), given.flag(2).unwrap_or(false), filters(given.json(3)));
    let request = OpenRequest { title, multiple, directory: false, filters };
    let platform = crate::host::rt::current().platform();
    let choose = move || -> ChosenFiles {
        let Some(paths) = platform.open_files(request)? else { return Ok(None) };
        Ok(Some(Files { owner, files: opened(paths, false)?, save: false }))
    };
    offload::run(scope, &mut rv, "acde-picker", choose, lend_files);
}

pub fn pick_save(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let mut given = bind::Args { scope: &mut *scope, args: &args };
    let (owner, title, name, filters) = (given.string(0), given.string(1), given.string(2), filters(given.json(3)));
    let request = SaveRequest { title, name, filters };
    let platform = crate::host::rt::current().platform();
    let choose = move || -> ChosenFiles {
        let Some(path) = platform.save_file(request)? else { return Ok(None) };
        Ok(Some(Files { owner, files: opened(vec![path], true)?, save: true }))
    };
    offload::run(scope, &mut rv, "acde-picker", choose, lend_files);
}

fn lend_files(scope: &mut v8::PinScope, resolver: v8::Local<v8::PromiseResolver>, chosen: ChosenFiles) {
    let Ok(Some(Files { owner, files, save })) = chosen else {
        let picked = chosen.map(|_| v8::null(scope).into());
        return answer(scope, resolver, picked);
    };
    let mut lent = Vec::new();
    for file in files {
        let (label, writable) = (file.label.clone(), file.writable);
        match crate::host::web::lend_file(&owner, file) {
            Ok(name) => lent.push(crate::js_object!(scope, {"name" => name, "label" => label, "writable" => writable}).into()),
            Err(error) => return answer(scope, resolver, Err(error.message)),
        }
    }
    let picked = match save {
        true => lent.first().copied().unwrap_or_else(|| v8::null(scope).into()),
        false => {
            let list = v8::Array::new_with_elements(scope, &lent);
            crate::js_object!(scope, {"files" => v8::Local::<v8::Value>::from(list)}).into()
        }
    };
    answer(scope, resolver, Ok(picked));
}

fn answer(scope: &mut v8::PinScope, resolver: v8::Local<v8::PromiseResolver>, picked: Result<v8::Local<v8::Value>, String>) {
    match picked {
        Ok(value) => {
            resolver.resolve(scope, value);
        }
        Err(message) => {
            let text = bind::string(scope, &message);
            let error = v8::Exception::error(scope, text);
            resolver.reject(scope, error);
        }
    }
}
