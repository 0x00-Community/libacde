use std::path::PathBuf;

use super::Assets;
use crate::render::Resource;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("acde-assets-{name}-{}", std::process::id()));
        _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("lent")).unwrap();
        std::fs::create_dir_all(dir.join("outside")).unwrap();
        std::fs::write(dir.join("lent/ok.mp4"), b"ok").unwrap();
        std::fs::write(dir.join("outside/secret"), b"secret").unwrap();
        Self(dir)
    }

    fn assets(&self) -> Assets {
        Assets(vec![Resource { prefix: "/media/".into(), dirs: vec![self.0.join("lent").to_string_lossy().into_owned()] }])
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_lent_file_is_found_and_dot_dot_is_not() {
    let scratch = Scratch::new("plain");
    let assets = scratch.assets();
    assert!(assets.file("http://acde.local/media/ok.mp4", false).is_some());
    assert!(assets.file("http://acde.local/media/../outside/secret", false).is_none());
    assert!(assets.file("http://other.example/media/ok.mp4", false).is_none());
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_out_of_the_lent_folder_is_not_followed() {
    let scratch = Scratch::new("link");
    std::os::unix::fs::symlink(scratch.0.join("outside/secret"), scratch.0.join("lent/escape")).unwrap();
    std::os::unix::fs::symlink(scratch.0.join("outside"), scratch.0.join("lent/dir")).unwrap();
    std::os::unix::fs::symlink(scratch.0.join("lent/ok.mp4"), scratch.0.join("lent/alias.mp4")).unwrap();
    let assets = scratch.assets();
    assert!(assets.file("http://acde.local/media/escape", false).is_none());
    assert!(assets.file("http://acde.local/media/dir/secret", false).is_none());
    assert!(assets.file("http://acde.local/media/alias.mp4", false).is_some(), "a link that stays inside is fine");
}
