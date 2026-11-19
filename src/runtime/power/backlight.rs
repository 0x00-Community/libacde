use std::path::{Path, PathBuf};

pub struct Backlight {
    pub name: String,
    dir: PathBuf,
    max: u32,
}

impl Backlight {
    const ROOT: &str = "/sys/class/backlight";

    pub fn find() -> Option<Self> {
        let rank = |kind: &str| match kind {
            "firmware" => 0,
            "platform" => 1,
            _ => 2,
        };
        let read = |dir: &Path, name: &str| std::fs::read_to_string(dir.join(name)).ok().map(|t| t.trim().to_owned());
        std::fs::read_dir(Self::ROOT)
            .ok()?
            .flatten()
            .filter_map(|entry| {
                let dir = entry.path();
                let max = read(&dir, "max_brightness")?.parse::<u32>().ok().filter(|m| *m > 0)?;
                let kind = read(&dir, "type").unwrap_or_default();
                Some((rank(&kind), std::cmp::Reverse(max), Self { name: entry.file_name().to_string_lossy().into_owned(), dir, max }))
            })
            .min_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)))
            .map(|(_, _, light)| light)
    }

    fn raw(&self) -> Option<u32> {
        ["actual_brightness", "brightness"].iter().find_map(|name| std::fs::read_to_string(self.dir.join(name)).ok()?.trim().parse().ok())
    }

    pub fn get(&self) -> Option<f64> {
        self.raw().map(|raw| (f64::from(raw) / f64::from(self.max)).clamp(0.0, 1.0))
    }

    pub fn set(&self, bus: &zbus::blocking::Connection, value: f64) -> Result<(), String> {
        if !value.is_finite() {
            return Err(crate::runtime::bad_request("acde.power.setBrightness: the value is 0..1"));
        }
        let lowest = self.max.div_ceil(100).max(1);
        let raw = ((value.clamp(0.0, 1.0) * f64::from(self.max)).round() as u32).clamp(lowest, self.max);
        super::bus::set_brightness(bus, &self.name, raw)
    }
}

pub fn brightness() -> Option<f64> {
    Backlight::find()?.get()
}

pub fn set_brightness(bus: &zbus::blocking::Connection, value: f64) -> Result<(), String> {
    Backlight::find().ok_or("this computer has no backlight")?.set(bus, value)
}
