use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Weak};

use blitz_traits::net::{Bytes, NetHandler, NetProvider, Request, Url};

use crate::render::Resource;

type Done = Box<dyn FnOnce(Option<Vec<u8>>, bool) + Send>;

pub struct Load {
    pub url: String,
    pub destination: &'static str,
    pub cors: &'static str,
    pub served: bool,
    pub cached: bool,
    done: Option<Done>,
}

impl Load {
    pub fn finish(mut self, bytes: Option<Vec<u8>>) {
        if let Some(done) = self.done.take() {
            done(bytes, false);
        }
    }

    pub fn finish_opaque(mut self, bytes: Vec<u8>) {
        if let Some(done) = self.done.take() {
            done(Some(bytes), true);
        }
    }
}

impl Drop for Load {
    fn drop(&mut self) {
        if let Some(done) = self.done.take() {
            done(None, false);
        }
    }
}

#[derive(Clone)]
pub struct Loader(Arc<dyn Fn(Load) + Send + Sync>);

impl Loader {
    pub fn new(load: impl Fn(Load) + Send + Sync + 'static) -> Self {
        Self(Arc::new(load))
    }
}

impl std::fmt::Debug for Loader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Loader")
    }
}

pub(crate) struct LocalNet {
    pub(crate) base: Url,
    pub(crate) resources: Vec<Resource>,
    pub(crate) arrived: Arc<AtomicBool>,
    loader: Option<Loader>,
    wake: Weak<Sender<()>>,
}

impl LocalNet {
    pub(crate) fn new(base: Url, resources: &[Resource], arrived: Arc<AtomicBool>, loader: Option<Loader>, wake: Weak<Sender<()>>) -> Self {
        let mut resources = resources.to_vec();
        resources.sort_by_key(|r| std::cmp::Reverse(r.prefix.len()));
        Self { base, resources, arrived, loader, wake }
    }

    fn path(&self, url: &Url) -> Option<PathBuf> {
        if url.scheme() == "file" {
            return self.file_path(url);
        }
        if url.origin() != self.base.origin() {
            return None;
        }
        let path = url.path();
        if let Some(file) = system_file(path) {
            return Some(file);
        }
        let (resource, rest) = self.resources.iter().find_map(|r| Some((r, path.strip_prefix(r.prefix.as_str())?)))?;
        resource_file(&resource.dirs, rest)
    }
}

const ICONS: &str = "/acde-icons/";
const SOUNDS: &str = "/acde-sounds/";

impl LocalNet {
    fn file_path(&self, url: &Url) -> Option<PathBuf> {
        if !crate::runtime::dev_mode() {
            return None;
        }
        let path = url.to_file_path().ok()?;
        self.resources.iter().flat_map(|r| r.dirs.iter().map(PathBuf::from)).find_map(|d| within(&d, &path))
    }
}

pub(crate) fn system_file(path: &str) -> Option<PathBuf> {
    let (rest, dirs) = match (path.strip_prefix(ICONS), path.strip_prefix(SOUNDS)) {
        (Some(rest), _) => (rest, icon_dirs()),
        (_, Some(rest)) => (rest, sound_dirs()),
        _ => return None,
    };
    system_file_among(&dirs, rest)
}

fn system_file_among(dirs: &[PathBuf], rest: &str) -> Option<PathBuf> {
    let path = Path::new("/").join(decoded(rest)?);
    dirs.iter().find_map(|d| within(d, &path))
}

pub(crate) fn resource_file(dirs: &[String], rest: &str) -> Option<PathBuf> {
    let rest = decoded(rest)?;
    if rest.components().any(|c| c.as_os_str().to_string_lossy().starts_with('.')) {
        return None;
    }
    dirs.iter().find_map(|d| within(Path::new(d), &Path::new(d).join(&rest)))
}

fn decoded(rest: &str) -> Option<PathBuf> {
    let path = Url::parse(&format!("file:///{}", rest.trim_start_matches('/'))).ok()?.to_file_path().ok()?;
    let rest = path.strip_prefix("/").ok()?.to_path_buf();
    (rest.components().next().is_some() && rest.components().all(|c| matches!(c, Component::Normal(_)))).then_some(rest)
}

fn shares() -> (Option<PathBuf>, Vec<PathBuf>) {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let data_home = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| home.as_ref().map(|h| h.join(".local/share")));
    let data_dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    (home, data_home.into_iter().chain(data_dirs.split(':').filter(|d| !d.is_empty()).map(PathBuf::from)).collect())
}

fn sound_dirs() -> Vec<PathBuf> {
    shares().1.into_iter().map(|s| s.join("sounds")).collect()
}

fn icon_dirs() -> Vec<PathBuf> {
    let (home, shares) = shares();
    let mut dirs: Vec<PathBuf> = shares.into_iter().flat_map(|s| [s.join("icons"), s.join("pixmaps"), s.join("swcatalog/icons")]).collect();
    dirs.extend(home.map(|h| h.join(".icons")));
    dirs
}

pub(crate) fn within(dir: &Path, path: &Path) -> Option<PathBuf> {
    let root = std::fs::canonicalize(dir).ok()?;
    let real = std::fs::canonicalize(path).ok()?;
    (real.starts_with(&root) && real.is_file()).then_some(real)
}

impl NetProvider for LocalNet {
    fn reused(&self, url: &Url, destination: &'static str) {
        if let Some(loader) = &self.loader
            && matches!(url.scheme(), "http" | "https" | "blob")
        {
            (loader.0)(Load { url: url.to_string(), destination, cors: "", served: true, cached: true, done: None });
        }
    }

    fn fetch(&self, _doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        let url = request.url;
        let bytes = match url.scheme() {
            "data" => data_url::DataUrl::process(url.as_str()).ok().and_then(|d| d.decode_to_vec().ok()).map(|(bytes, _)| bytes),
            _ => self.path(&url).and_then(|p| std::fs::read(p).ok()),
        };
        if let (None, Some(loader)) = (&bytes, &self.loader)
            && matches!(url.scheme(), "http" | "https" | "blob")
        {
            let (arrived, wake, resolved) = (self.arrived.clone(), self.wake.clone(), url.to_string());
            let (destination, cors) = (handler.destination(), handler.cors());
            let mut handler = handler;
            let done: Done = Box::new(move |bytes, opaque| {
                if opaque {
                    handler.opaque();
                }
                match bytes {
                    Some(bytes) => handler.bytes(resolved, Bytes::from(bytes)),
                    None => handler.failed(resolved),
                }
                arrived.store(true, Ordering::Release);
                if let Some(wake) = wake.upgrade() {
                    _ = wake.send(());
                }
            });
            return (loader.0)(Load { url: url.to_string(), destination, cors, served: false, cached: false, done: Some(done) });
        }
        if let (Some(_), Some(loader)) = (&bytes, &self.loader)
            && matches!(url.scheme(), "http" | "https")
        {
            (loader.0)(Load { url: url.to_string(), destination: handler.destination(), cors: handler.cors(), served: true, cached: false, done: None });
        }
        self.arrived.store(true, Ordering::Release);
        match bytes {
            Some(bytes) => handler.bytes(url.to_string(), Bytes::from(bytes)),
            None => {
                log::warn!("not loaded: {url}");
                handler.failed(url.to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("acde-net-{name}-{}", std::process::id()));
            _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("lent")).unwrap();
            std::fs::create_dir_all(dir.join("outside")).unwrap();
            std::fs::write(dir.join("lent/ok.png"), b"ok").unwrap();
            std::fs::write(dir.join("outside/secret"), b"secret").unwrap();
            Self(dir)
        }

        fn net(&self) -> LocalNet {
            let lent = self.0.join("lent").to_string_lossy().into_owned();
            let resources = [Resource { prefix: "/media/".into(), dirs: vec![lent] }];
            LocalNet::new(Url::parse("http://acde.local/").unwrap(), &resources, Arc::default(), None, Weak::new())
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    #[test]
    fn a_lent_file_is_read_and_dot_dot_is_not() {
        let scratch = Scratch::new("plain");
        let net = scratch.net();
        assert!(net.path(&url("http://acde.local/media/ok.png")).is_some());
        std::fs::write(scratch.0.join("lent/a b.png"), b"ab").unwrap();
        assert!(net.path(&url("http://acde.local/media/a%20b.png")).is_some(), "the path is percent-decoded");
        assert!(net.path(&url("http://acde.local/media/%2e%2e/outside/secret")).is_none());
        std::fs::write(scratch.0.join("lent/.env"), b"secret").unwrap();
        assert!(net.path(&url("http://acde.local/media/.env")).is_none(), "a hidden file is not served");
        assert!(net.path(&url("http://acde.local/media/")).is_none());
        assert!(net.path(&url("http://acde.local/media/../outside/secret")).is_none());
        assert!(net.path(&url("http://other.example/media/ok.png")).is_none());
    }

    #[test]
    fn a_file_url_is_refused_outside_dev_mode() {
        let scratch = Scratch::new("file");
        let net = scratch.net();
        let file = |path: &Path| Url::from_file_path(path).unwrap();
        if !crate::runtime::dev_mode() {
            assert!(net.path(&file(&scratch.0.join("lent/ok.png"))).is_none());
        }
        assert!(net.path(&file(&scratch.0.join("outside/secret"))).is_none());
        assert!(net.path(&url("file:///etc/passwd")).is_none());
    }

    #[test]
    fn acde_icons_reads_only_files_under_an_icon_folder() {
        let scratch = Scratch::new("icons");
        let net = scratch.net();
        let icons = |path: &Path| url(&format!("http://acde.local/acde-icons{}", path.display()));
        assert!(net.path(&url("http://acde.local/acde-icons/etc/passwd")).is_none());
        assert!(net.path(&icons(&scratch.0.join("lent/ok.png"))).is_none(), "a lent folder is not an icon folder");
        assert!(net.path(&url("http://acde.local/acde-icons/usr/share/icons/../../../etc/passwd")).is_none());
        assert!(net.path(&url("http://other.example/acde-icons/usr/share/icons/x.png")).is_none());
        let dir = scratch.0.join("share/icons/hi color");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("图标.png"), b"png").unwrap();
        let encoded = Url::from_file_path(dir.join("图标.png")).unwrap().path().trim_start_matches('/').to_owned();
        assert!(encoded.contains("%20") && encoded.contains("%E5"), "{encoded}");
        let decoded = system_file_among(&[scratch.0.join("share/icons")], &encoded);
        assert_eq!(decoded, std::fs::canonicalize(dir.join("图标.png")).ok(), "a percent-encoded path is decoded first");
        let hidden = scratch.0.join(".icons");
        std::fs::create_dir_all(&hidden).unwrap();
        std::fs::write(hidden.join("x.png"), b"png").unwrap();
        let rest = hidden.join("x.png").to_string_lossy().trim_start_matches('/').to_owned();
        assert!(system_file_among(&[hidden.clone()], &rest).is_some(), "~/.icons is an icon folder, its dot is fine");
        let icon = icon_dirs().into_iter().filter_map(|d| std::fs::read_dir(d).ok()).flatten().flatten().map(|e| e.path()).find(|p| p.is_file());
        if let Some(icon) = icon {
            assert_eq!(net.path(&icons(&icon)), std::fs::canonicalize(&icon).ok(), "an icon under an icon folder: {}", icon.display());
        }
    }

    #[test]
    fn acde_sounds_reads_only_files_under_a_sounds_folder() {
        let scratch = Scratch::new("sounds");
        let net = scratch.net();
        assert!(net.path(&url("http://acde.local/acde-sounds/etc/passwd")).is_none());
        assert!(net.path(&url("http://acde.local/acde-sounds/usr/share/sounds/../../../etc/passwd")).is_none());
        let dir = scratch.0.join("share/sounds/freedesktop/stereo");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("bell.oga"), b"ogg").unwrap();
        let rest = dir.join("bell.oga").to_string_lossy().trim_start_matches('/').to_owned();
        let sounds = [scratch.0.join("share/sounds")];
        assert_eq!(system_file_among(&sounds, &rest), std::fs::canonicalize(dir.join("bell.oga")).ok());
        assert!(system_file_among(&[scratch.0.join("share/icons")], &rest).is_none(), "a sound is not under an icon folder");
        assert!(sound_dirs().iter().all(|d| d.ends_with("sounds")));
    }

    #[cfg(unix)]
    #[test]
    fn a_symbolic_link_out_of_the_lent_folder_is_not_followed() {
        let scratch = Scratch::new("link");
        std::os::unix::fs::symlink(scratch.0.join("outside/secret"), scratch.0.join("lent/escape")).unwrap();
        std::os::unix::fs::symlink(scratch.0.join("outside"), scratch.0.join("lent/dir")).unwrap();
        std::os::unix::fs::symlink(scratch.0.join("lent/ok.png"), scratch.0.join("lent/alias.png")).unwrap();
        let net = scratch.net();
        assert!(net.path(&url("http://acde.local/media/escape")).is_none());
        assert!(net.path(&url("http://acde.local/media/dir/secret")).is_none());
        assert!(net.path(&url("http://acde.local/media/alias.png")).is_some(), "a link that stays inside is fine");
    }
}
