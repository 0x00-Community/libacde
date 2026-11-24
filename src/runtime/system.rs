use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use zbus::blocking::{Connection, Proxy};

pub enum SysCall {
    Info,
    Storage,
    Time,
    Timezones,
    SetTimezone { zone: String },
    SetNtp { on: bool },
    SetHostname { name: String },
    User,
}

pub fn system_call(call: SysCall) -> Result<Value, String> {
    match call {
        SysCall::Info => Ok(info()),
        SysCall::Storage => Ok(storage()),
        SysCall::Time => Ok(time()),
        SysCall::Timezones => timezones(),
        SysCall::SetTimezone { zone } => set_timezone(&zone),
        SysCall::SetNtp { on } => timedate()?.call::<_, _, ()>("SetNTP", &(on, false)).map(|()| Value::Null).map_err(said),
        SysCall::SetHostname { name } => set_hostname(&name),
        SysCall::User => Ok(user()),
    }
}

const SKIP_GIB: u64 = 1 << 30;

fn said(error: zbus::Error) -> String {
    match error {
        zbus::Error::MethodError(_, Some(text), _) => text,
        other => other.to_string(),
    }
}

fn read(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
}

fn os_release() -> Option<String> {
    let text = fs::read_to_string("/etc/os-release").or_else(|_| fs::read_to_string("/usr/lib/os-release")).ok()?;
    let field = |key: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
            .map(|v| v.trim().trim_matches('"').trim_matches('\'').to_owned())
            .filter(|v| !v.is_empty())
    };
    field("PRETTY_NAME").or_else(|| field("NAME"))
}

fn cpu() -> (String, usize) {
    let text = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let model = text
        .lines()
        .find_map(|line| line.strip_prefix("model name").and_then(|rest| rest.split_once(':')).map(|(_, v)| v.split_whitespace().collect::<Vec<_>>().join(" ")))
        .unwrap_or_default();
    let cores = text.lines().filter(|line| line.starts_with("processor")).count();
    let cores = if cores > 0 { cores } else { std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0) };
    (model, cores)
}

fn memory() -> u64 {
    fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|text| text.lines().find_map(|line| line.strip_prefix("MemTotal:")?.trim().strip_suffix("kB")?.trim().parse::<u64>().ok()))
        .map_or(0, |kib| kib * 1024)
}

fn uptime() -> u64 {
    read("/proc/uptime").and_then(|text| text.split_whitespace().next()?.parse::<f64>().ok()).map_or(0, |s| s as u64)
}

fn serial() -> Option<String> {
    let serial = read("/sys/class/dmi/id/product_serial")?;
    let junk = ["to be filled", "default string", "system serial number", "not specified", "none", "0", "123456789"];
    (!junk.contains(&serial.to_lowercase().as_str())).then_some(serial)
}

struct PciIds {
    text: String,
}

impl PciIds {
    fn load() -> Self {
        let text =
            ["/usr/share/hwdata/pci.ids", "/usr/share/misc/pci.ids", "/usr/share/pci.ids"].iter().find_map(|p| fs::read_to_string(p).ok()).unwrap_or_default();
        PciIds { text }
    }

    fn lookup(&self, vendor: u16, device: u16) -> (Option<&str>, Option<&str>) {
        let mut lines = self.text.lines().skip_while(|line| line.starts_with('\t') || pci_entry(line).is_none_or(|(id, _)| id != vendor));
        let Some(vendor_name) = lines.next().and_then(pci_entry).map(|(_, name)| name) else { return (None, None) };
        let device_name = lines
            .take_while(|line| line.is_empty() || line.starts_with('#') || line.starts_with('\t'))
            .filter_map(|line| line.strip_prefix('\t').filter(|rest| !rest.starts_with('\t')))
            .find_map(|rest| pci_entry(rest).filter(|(id, _)| *id == device).map(|(_, name)| name));
        (Some(vendor_name), device_name)
    }

    fn name(&self, vendor: u16, device: u16) -> String {
        let short = match vendor {
            0x8086 => Some("Intel"),
            0x1002 | 0x1022 => Some("AMD"),
            0x10de => Some("NVIDIA"),
            _ => None,
        };
        let (known, device_name) = self.lookup(vendor, device);
        let vendor_name = short.or(known).map_or_else(|| format!("{vendor:04x}"), str::to_owned);
        match device_name {
            Some(device) => format!("{vendor_name} {device}"),
            None => format!("{vendor_name} {device:04x}"),
        }
    }
}

fn pci_entry(line: &str) -> Option<(u16, &str)> {
    let (id, name) = line.split_once("  ")?;
    Some((u16::from_str_radix(id.trim(), 16).ok()?, name.trim()))
}

fn pci_id(dir: &Path, file: &str) -> Option<u16> {
    u16::from_str_radix(read(dir.join(file))?.trim_start_matches("0x"), 16).ok()
}

fn devices(class: &str, keep: impl Fn(&str) -> bool, ids: &PciIds) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut names = Vec::new();
    let Ok(entries) = fs::read_dir(class) else { return names };
    let mut entries: Vec<_> = entries.flatten().filter(|e| keep(&e.file_name().to_string_lossy())).map(|e| e.path()).collect();
    entries.sort();
    for entry in entries {
        let device = entry.join("device");
        let Ok(real) = fs::canonicalize(&device) else { continue };
        if !seen.insert(real) {
            continue;
        }
        if let (Some(vendor), Some(id)) = (pci_id(&device, "vendor"), pci_id(&device, "device")) {
            names.push(ids.name(vendor, id));
        }
    }
    names
}

fn info() -> Value {
    let (cpu, cores) = cpu();
    let ids = PciIds::load();
    let gpu = devices("/sys/class/drm", |name| name.starts_with("card") && !name.contains('-'), &ids);
    let npu = devices("/sys/class/accel", |name| name.starts_with("accel"), &ids).into_iter().next();
    json!({
        "hostname": read("/proc/sys/kernel/hostname").unwrap_or_default(),
        "os": os_release().unwrap_or_else(|| "Linux".to_owned()),
        "kernel": read("/proc/sys/kernel/osrelease").unwrap_or_default(),
        "cpu": cpu,
        "cores": cores,
        "memory": memory(),
        "gpu": gpu,
        "npu": npu,
        "uptimeSeconds": uptime(),
        "serial": serial(),
    })
}

fn unescape(text: &str, prefix: &str, digits: usize, radix: u32) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let code = i + prefix.len() + digits;
        if bytes[i..].starts_with(prefix.as_bytes())
            && code <= bytes.len()
            && let Some(digits) = text.get(i + prefix.len()..code)
            && let Ok(n) = u8::from_str_radix(digits, radix)
        {
            out.push(n);
            i = code;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn labels() -> HashMap<PathBuf, String> {
    let mut labels = HashMap::new();
    let Ok(entries) = fs::read_dir("/dev/disk/by-label") else { return labels };
    for entry in entries.flatten() {
        if let Ok(device) = fs::canonicalize(entry.path()) {
            labels.insert(device, unescape(&entry.file_name().to_string_lossy(), "\\x", 2, 16));
        }
    }
    labels
}

struct Mount {
    source: String,
    path: String,
}

fn rank(path: &str) -> (u8, usize) {
    match path {
        "/" => (0, 0),
        "/home" => (1, 0),
        _ => (2, path.len()),
    }
}

fn mounts() -> Vec<Mount> {
    let text = fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
    let mut best: HashMap<String, Mount> = HashMap::new();
    let mut order = Vec::new();
    for line in text.lines() {
        let Some((head, tail)) = line.split_once(" - ") else { continue };
        let head: Vec<&str> = head.split(' ').collect();
        let tail: Vec<&str> = tail.split(' ').collect();
        let (Some(path), Some(fstype), Some(source)) = (head.get(4), tail.first(), tail.get(1)) else { continue };
        let path = unescape(path, "\\", 3, 8);
        let source = unescape(source, "\\", 3, 8);
        if !source.starts_with("/dev/") || ["/dev/loop", "/dev/zram", "/dev/ram", "/dev/mapper/control"].iter().any(|p| source.starts_with(p)) {
            continue;
        }
        if matches!(*fstype, "squashfs" | "iso9660" | "udf" | "swap") {
            continue;
        }
        if ["/boot", "/efi", "/snap/", "/var/lib/docker", "/var/lib/containers"].iter().any(|p| path == p.trim_end_matches('/') || path.starts_with(p)) {
            continue;
        }
        let source = fs::canonicalize(&source).map(|p| p.to_string_lossy().into_owned()).unwrap_or(source);
        match best.get_mut(&source) {
            Some(kept) if rank(&path) < rank(&kept.path) => kept.path = path,
            Some(_) => {}
            None => {
                order.push(source.clone());
                best.insert(source.clone(), Mount { source, path });
            }
        }
    }
    let mut mounts: Vec<Mount> = order.into_iter().filter_map(|s| best.remove(&s)).collect();
    mounts.sort_by(|a, b| rank(&a.path).cmp(&rank(&b.path)).then_with(|| a.path.cmp(&b.path)));
    mounts
}

fn storage() -> Value {
    let labels = labels();
    let volumes: Vec<Value> = mounts()
        .into_iter()
        .filter_map(|m| {
            let stat = rustix::fs::statvfs(m.path.as_str()).ok()?;
            let size = stat.f_blocks.saturating_mul(stat.f_frsize);
            let free = stat.f_bavail.saturating_mul(stat.f_frsize);
            (size >= SKIP_GIB).then(|| {
                let label = labels.get(Path::new(&m.source)).cloned().unwrap_or_default();
                json!({ "path": m.path, "label": label, "size": size, "free": free })
            })
        })
        .collect();
    Value::Array(volumes)
}

fn system_bus() -> Result<Connection, String> {
    Connection::system().map_err(said)
}

fn timedate() -> Result<Proxy<'static>, String> {
    let bus = system_bus()?;
    Proxy::new(&bus, "org.freedesktop.timedate1", "/org/freedesktop/timedate1", "org.freedesktop.timedate1").map_err(said)
}

fn local_zone() -> String {
    fs::read_link("/etc/localtime")
        .ok()
        .and_then(|target| {
            let target = target.to_string_lossy().into_owned();
            target.split_once("zoneinfo/").map(|(_, zone)| zone.to_owned())
        })
        .or_else(|| read("/etc/timezone"))
        .unwrap_or_else(|| "UTC".to_owned())
}

fn time() -> Value {
    match timedate() {
        Ok(proxy) => {
            let zone = proxy.get_property::<String>("Timezone").ok().filter(|z| !z.is_empty()).unwrap_or_else(local_zone);
            let ntp = proxy.get_property::<bool>("NTP").unwrap_or(false);
            let can_ntp = proxy.get_property::<bool>("CanNTP").unwrap_or(false);
            json!({ "timezone": zone, "ntp": ntp, "canSetTime": can_ntp })
        }
        Err(_) => json!({ "timezone": local_zone(), "ntp": false, "canSetTime": false }),
    }
}

fn timezones() -> Result<Value, String> {
    let zones: Vec<String> = timedate()?.call("ListTimezones", &()).map_err(said)?;
    Ok(json!(zones))
}

fn set_timezone(zone: &str) -> Result<Value, String> {
    if zone.is_empty() || zone.contains("..") {
        return Err(crate::runtime::bad_request(format!("not a time zone: {zone}")));
    }
    timedate()?.call::<_, _, ()>("SetTimezone", &(zone, false)).map(|()| Value::Null).map_err(said)
}

fn set_hostname(name: &str) -> Result<Value, String> {
    let name = name.trim();
    let valid = !name.is_empty()
        && name.len() <= 64
        && name
            .split('.')
            .all(|part| !part.is_empty() && !part.starts_with('-') && !part.ends_with('-') && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    if !valid {
        return Err(crate::runtime::bad_request(format!("not a host name: {name}")));
    }
    let bus = system_bus()?;
    let proxy = Proxy::new(&bus, "org.freedesktop.hostname1", "/org/freedesktop/hostname1", "org.freedesktop.hostname1").map_err(said)?;
    proxy.call::<_, _, ()>("SetStaticHostname", &(name, false)).map_err(said)?;
    proxy.call::<_, _, ()>("SetHostname", &(name, false)).map_err(said)?;
    Ok(Value::Null)
}

fn passwd(uid: u32) -> Option<(String, String)> {
    let text = fs::read_to_string("/etc/passwd").ok()?;
    text.lines().find_map(|line| {
        let fields: Vec<&str> = line.split(':').collect();
        (fields.get(2)?.parse::<u32>().ok()? == uid)
            .then(|| (fields[0].to_owned(), fields.get(4).map(|g| g.split(',').next().unwrap_or("").to_owned()).unwrap_or_default()))
    })
}

fn accounts(uid: u32) -> Option<(String, String, Option<String>)> {
    let bus = system_bus().ok()?;
    let manager = Proxy::new(&bus, "org.freedesktop.Accounts", "/org/freedesktop/Accounts", "org.freedesktop.Accounts").ok()?;
    let path: zbus::zvariant::OwnedObjectPath = manager.call("FindUserById", &(u64::from(uid),)).ok()?;
    let user = Proxy::new(&bus, "org.freedesktop.Accounts", path, "org.freedesktop.Accounts.User").ok()?;
    let name = user.get_property::<String>("UserName").ok()?;
    let full = user.get_property::<String>("RealName").unwrap_or_default();
    let icon = user.get_property::<String>("IconFile").ok().filter(|p| !p.is_empty() && Path::new(p).is_file());
    Some((name, full, icon))
}

fn user() -> Value {
    let uid = rustix::process::getuid().as_raw();
    let (name, full, icon) = accounts(uid).or_else(|| passwd(uid).map(|(name, full)| (name, full, None))).unwrap_or_else(|| {
        let name = std::env::var("USER").unwrap_or_default();
        (name, String::new(), None)
    });
    let full = if full.is_empty() { name.clone() } else { full };
    json!({ "name": name, "fullName": full, "avatar": icon })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_machine() {
        let volumes = storage();
        println!("{volumes}");
        let volumes = volumes.as_array().cloned().unwrap_or_default();
        assert!(volumes.iter().any(|v| v["path"] == "/"));
        assert!(volumes.iter().all(|v| v["size"].as_u64() >= v["free"].as_u64()));
        let info = info();
        println!("{info}");
        assert!(info["cores"].as_u64().unwrap_or(0) > 0);
        assert!(info["memory"].as_u64().unwrap_or(0) > 0);
        println!("{}", time());
        println!("{}", user());
    }

    #[test]
    fn pci_names_come_from_the_vendor_and_its_own_devices() {
        let ids = PciIds {
            text: "# comment\n1234  Acme\n\t0001  Widget\n\t\t1234 0001  Widget Pro\n\n8086  Intel Corporation\n\t0001  Other\n\t7d67  Arc Graphics\nC 03  Display controller\n\t00  VGA\n".into(),
        };
        assert_eq!(ids.name(0x1234, 0x0001), "Acme Widget");
        assert_eq!(ids.name(0x8086, 0x7d67), "Intel Arc Graphics");
        assert_eq!(ids.name(0x8086, 0x0002), "Intel 0002");
        assert_eq!(ids.name(0x1234, 0x7d67), "Acme 7d67");
        assert_eq!(ids.name(0x4321, 0x0001), "4321 0001");
    }

    #[test]
    fn escapes() {
        assert_eq!(unescape("/mnt/my\\040disk", "\\", 3, 8), "/mnt/my disk");
        assert_eq!(unescape("Data\\x20Disk", "\\x", 2, 16), "Data Disk");
        assert_eq!(unescape("数据\\040盘", "\\", 3, 8), "数据 盘");
    }
}
