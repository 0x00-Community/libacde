use super::workspace::{FinderRoot, RootMode, RootSource};
use std::collections::HashMap;
use std::path::Path;

pub const CAPABILITY: &str = "removable";
pub const PREFIX: &str = "removable-";
const MOUNTINFO: &str = "/proc/self/mountinfo";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mount {
    pub path: String,
    pub device: String,
    pub uuid: Option<String>,
    pub label: Option<String>,
    pub read_only: bool,
}

impl Mount {
    pub fn id(&self) -> String {
        format!("{PREFIX}{}", self.uuid.as_deref().unwrap_or(&self.device))
    }

    pub fn root(&self, shown: bool) -> FinderRoot {
        let label = self.label.as_deref().unwrap_or(&self.device);
        let exists = Path::new(&self.path).is_dir();
        let mode = match (exists, self.read_only) {
            (false, _) => RootMode::Unavailable,
            (true, true) => RootMode::ReadOnly,
            (true, false) => RootMode::Writable,
        };
        FinderRoot {
            id: self.id(),
            shown: shown.then(|| self.path.clone()),
            mode,
            exists,
            reason: (!exists).then(|| "the drive is no longer mounted".to_owned()),
            drive: Some(self.device.clone()),
            ..FinderRoot::new(self.path.clone(), label, RootSource::Removable)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    path: String,
    source: String,
    read_only: bool,
}

#[derive(Default)]
struct Disks {
    uuids: HashMap<String, String>,
    labels: HashMap<String, String>,
}

impl Disks {
    fn read() -> Self {
        Self { uuids: Self::links("/dev/disk/by-uuid"), labels: Self::links("/dev/disk/by-label") }
    }

    fn links(dir: &str) -> HashMap<String, String> {
        let Ok(entries) = std::fs::read_dir(dir) else { return HashMap::new() };
        entries
            .flatten()
            .filter_map(|entry| {
                let name = unescape_udev(&entry.file_name().to_string_lossy());
                let device = std::fs::canonicalize(entry.path()).ok()?.file_name()?.to_string_lossy().into_owned();
                Some((device, name))
            })
            .collect()
    }

    fn mount(&self, entry: Entry) -> Mount {
        let real = std::fs::canonicalize(&entry.source).ok();
        let device = real.as_deref().unwrap_or(Path::new(&entry.source)).file_name().map(|name| name.to_string_lossy().into_owned());
        let device = device.unwrap_or_else(|| entry.source.clone());
        Mount { uuid: self.uuids.get(&device).cloned(), label: self.labels.get(&device).cloned(), path: entry.path, read_only: entry.read_only, device }
    }
}

pub fn mounted() -> Vec<Mount> {
    let Some(user) = user() else { return Vec::new() };
    let entries = entries(&std::fs::read_to_string(MOUNTINFO).unwrap_or_default(), &user);
    if entries.is_empty() {
        return Vec::new();
    }
    let disks = Disks::read();
    entries.into_iter().map(|entry| disks.mount(entry)).collect()
}

fn user() -> Option<String> {
    std::env::var("USER").ok().filter(|user| !user.is_empty() && !user.contains('/'))
}

fn entries(mountinfo: &str, user: &str) -> Vec<Entry> {
    let prefixes = [format!("/run/media/{user}/"), format!("/media/{user}/")];
    let mut found: Vec<Entry> = mountinfo.lines().filter_map(entry).filter(|entry| removable_at(&entry.path, &prefixes)).collect();
    found.sort_by(|a, b| a.path.cmp(&b.path));
    found.dedup_by(|a, b| a.path == b.path);
    found
}

fn entry(line: &str) -> Option<Entry> {
    let fields: Vec<&str> = line.split(' ').collect();
    let path = unescape_mount(fields.get(4)?);
    let read_only = fields.get(5)?.split(',').any(|option| option == "ro");
    let dash = fields.iter().position(|field| *field == "-")?;
    let source = unescape_mount(fields.get(dash + 2)?);
    source.starts_with("/dev/").then_some(Entry { path, source, read_only })
}

fn removable_at(path: &str, prefixes: &[String]) -> bool {
    prefixes.iter().any(|prefix| path.strip_prefix(prefix.as_str()).is_some_and(|rest| !rest.is_empty() && !rest.contains('/')))
}

pub(super) fn unescape_mount(field: &str) -> String {
    unescape(field, |bytes| {
        let digits = bytes.get(1..4).filter(|d| d.iter().all(|c| (b'0'..=b'7').contains(c)))?;
        let value = digits.iter().fold(0u32, |sum, digit| sum * 8 + u32::from(digit - b'0'));
        Some((u8::try_from(value).unwrap_or(b'?'), 4))
    })
}

fn unescape_udev(name: &str) -> String {
    unescape(name, |bytes| {
        let digits = bytes.get(1..4).filter(|d| d[0] == b'x')?;
        let hex = std::str::from_utf8(&digits[1..]).ok()?;
        Some((u8::from_str_radix(hex, 16).ok()?, 4))
    })
}

fn unescape(text: &str, escaped: impl Fn(&[u8]) -> Option<(u8, usize)>) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match Some(&bytes[i..]).filter(|rest| rest[0] == b'\\').and_then(&escaped) {
            Some((byte, width)) => {
                out.push(byte);
                i += width;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(target_os = "linux")]
fn ids() -> Vec<String> {
    mounted().iter().map(Mount::id).collect()
}

#[cfg(target_os = "linux")]
pub fn watch(changed: impl Fn(Vec<String>) + Send + 'static) -> std::io::Result<Vec<String>> {
    use rustix::event::{PollFd, PollFlags, poll};
    let file = std::fs::File::open(MOUNTINFO)?;
    let first = ids();
    let mut known = first.clone();
    std::thread::Builder::new().name("acde-mounts".into()).spawn(move || {
        loop {
            let mut fds = [PollFd::new(&file, PollFlags::PRI | PollFlags::ERR)];
            if poll(&mut fds, None).is_err() {
                return;
            }
            let now = ids();
            if now != known {
                known = now.clone();
                changed(now);
            }
        }
    })?;
    Ok(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOUNTINFO: &str = "\
22 1 8:2 / / rw,relatime shared:1 - ext4 /dev/sda2 rw
90 22 8:17 / /run/media/me/STICK rw,nosuid,nodev,relatime shared:50 - vfat /dev/sdb1 rw,fmask=0022
91 22 8:33 / /run/media/me/My\\040Card ro,nosuid,nodev shared:51 - exfat /dev/sdc1 ro
92 22 8:49 / /media/me/BACKUP rw,relatime shared:52 - ext4 /dev/sdd1 rw
93 22 8:65 / /mnt/data rw,relatime shared:53 - ext4 /dev/sde1 rw
94 22 0:60 / /run/media/me/share rw,nosuid shared:54 - fuse.sshfs host:/share rw
95 22 8:81 / /run/media/me/STICK/inner rw shared:55 - vfat /dev/sdf1 rw
96 22 8:97 / /run/media/other/THEIRS rw shared:56 - vfat /dev/sdg1 rw";

    #[test]
    fn only_block_devices_right_under_the_users_media_folders_count() {
        let paths: Vec<(String, bool)> = entries(MOUNTINFO, "me").into_iter().map(|entry| (entry.path, entry.read_only)).collect();
        let expected = [("/media/me/BACKUP", false), ("/run/media/me/My Card", true), ("/run/media/me/STICK", false)];
        assert_eq!(paths, expected.map(|(path, ro)| (path.to_owned(), ro)));
    }

    #[test]
    fn the_id_follows_the_volume_uuid_and_the_label_falls_back_to_the_device() {
        let mut disks = Disks::default();
        disks.uuids.insert("sdb1".into(), "1234-ABCD".into());
        disks.labels.insert("sdb1".into(), "STICK".into());
        let stick = disks.mount(Entry { path: "/run/media/me/STICK".into(), source: "/dev/sdb1".into(), read_only: false });
        assert_eq!((stick.id(), stick.label.as_deref(), stick.device.as_str()), ("removable-1234-ABCD".into(), Some("STICK"), "sdb1"));
        let bare = disks.mount(Entry { path: "/run/media/me/x".into(), source: "/dev/sdc1".into(), read_only: true });
        assert_eq!(bare.id(), "removable-sdc1");
        let root = serde_json::to_value(bare.root(false)).unwrap();
        assert_eq!(root["label"], "sdc1");
        assert_eq!(root["source"], "removable");
        assert_eq!(root["drive"], "sdc1");
        assert!(root.get("path").is_none());
    }

    #[test]
    fn a_mounted_folder_is_read_only_when_mounted_so() {
        let dir = std::env::temp_dir().join(format!("acde-removable-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.to_string_lossy().into_owned();
        let mount = |read_only| Mount { path: path.clone(), device: "sdb1".into(), uuid: None, label: None, read_only };
        assert_eq!(serde_json::to_value(mount(true).root(true)).unwrap()["mode"], "read_only");
        assert_eq!(serde_json::to_value(mount(false).root(true)).unwrap()["mode"], "writable");
        assert_eq!(serde_json::to_value(mount(false).root(true)).unwrap()["path"], path.as_str());
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(serde_json::to_value(mount(false).root(true)).unwrap()["mode"], "unavailable");
    }

    #[test]
    fn escapes_in_mount_points_and_udev_names_are_undone() {
        assert_eq!(unescape_mount("/run/media/me/My\\040Stick"), "/run/media/me/My Stick");
        assert_eq!(unescape_mount("/a\\134b\\9"), "/a\\b\\9");
        assert_eq!(unescape_udev("My\\x20Stick"), "My Stick");
        assert_eq!(unescape_udev("a\\xZZ"), "a\\xZZ");
    }
}
