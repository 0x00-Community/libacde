use std::sync::{Arc, Condvar, Mutex, Once, OnceLock};
use std::time::{Duration, Instant};

use crate::gpu::{Gpu, GpuImage, Offscreen, Scene, lock};
use kurbo::Affine;

const IDLE_TRIM: Duration = Duration::from_secs(2);

static OFFSCREEN: Mutex<Option<(Arc<Gpu>, Box<dyn Offscreen>)>> = Mutex::new(None);
static DRAWN: Mutex<Option<Instant>> = Mutex::new(None);
static DREW: Condvar = Condvar::new();
static TRIMMER: Once = Once::new();

pub type CpuRaster = Box<dyn Fn(Scene, (u32, u32)) -> Option<Vec<u8>> + Send + Sync>;

static CPU_RASTER: OnceLock<CpuRaster> = OnceLock::new();

#[derive(Debug, PartialEq)]
enum Held {
    Use,
    Rebuild,
    Wait,
}

/// What to do with the offscreen built on a device: use it while the device lives, wait while a lost device is still the
/// published one (the host window is opening a new one), rebuild once a new device is published.
fn held(gone: bool, still_published: impl FnOnce() -> bool) -> Held {
    match (gone, gone && still_published()) {
        (false, _) => Held::Use,
        (true, true) => Held::Wait,
        (true, false) => Held::Rebuild,
    }
}

fn with<T>(f: impl FnOnce(&mut dyn Offscreen) -> Result<T, String>) -> Option<T> {
    let mut slot = lock(&OFFSCREEN);
    if let Some((gpu, _)) = slot.as_ref() {
        match held(gpu.gone(), || Gpu::shared().is_some_and(|now| Arc::ptr_eq(&now, gpu))) {
            Held::Use => {}
            Held::Wait => return None,
            Held::Rebuild => *slot = None,
        }
    }
    if slot.is_none() {
        let gpu = Gpu::shared()?;
        let offscreen = gpu.offscreen().inspect_err(|e| unready(&gpu, e)).ok()?;
        *slot = Some((gpu, offscreen));
    }
    let (_, offscreen) = slot.as_mut()?;
    f(offscreen.as_mut()).inspect_err(|e| log::warn!("render: {e}")).ok()
}

fn drawing<T>(f: impl FnOnce(&mut dyn Offscreen) -> Result<T, String>) -> Option<T> {
    with(|offscreen| {
        let drawn = f(offscreen);
        *lock(&DRAWN) = Some(Instant::now());
        DREW.notify_one();
        TRIMMER.call_once(spawn_trimmer);
        drawn
    })
}

fn spawn_trimmer() {
    if let Err(e) = std::thread::Builder::new().name("acde-offscreen-trim".into()).spawn(trim_when_idle) {
        log::warn!("offscreen trim thread: {e}");
    }
}

fn trim_when_idle() {
    loop {
        let due = {
            let mut drawn = lock(&DRAWN);
            loop {
                match *drawn {
                    Some(at) => break at + IDLE_TRIM,
                    None => drawn = DREW.wait(drawn).unwrap_or_else(|e| e.into_inner()),
                }
            }
        };
        let now = Instant::now();
        if due > now {
            std::thread::sleep(due - now);
            continue;
        }
        let mut slot = lock(&OFFSCREEN);
        let mut drawn = lock(&DRAWN);
        if drawn.is_none_or(|at| at + IDLE_TRIM > Instant::now()) {
            continue;
        }
        *drawn = None;
        drop(drawn);
        if let Some((gpu, offscreen)) = slot.as_mut()
            && !gpu.gone()
            && let Err(e) = offscreen.trim()
        {
            log::warn!("render: dropping the offscreen vello's buffers: {e}");
        }
    }
}

fn unready(gpu: &Arc<Gpu>, error: &str) {
    if Gpu::get().flatten().is_some_and(|published| Arc::ptr_eq(&published, gpu)) {
        log::warn!("render: {error}");
    } else {
        log::debug!("render: no offscreen yet ({error}): the session's GPU is not published");
    }
}

pub fn warm() {
    let started = std::time::Instant::now();
    if with(|_| Ok(())).is_some() {
        log::info!("offscreen vello warm-up {:.1}ms", started.elapsed().as_secs_f64() * 1000.0);
    }
}

pub fn draw(scene: Scene, size: (u32, u32), place: Affine) -> Option<Arc<GpuImage>> {
    drawing(|o| o.draw(scene, size, place))
}

/// Reads `image` back; an image of a replaced device has nothing to read (None).
pub fn read(image: &GpuImage, rect: [u32; 4]) -> Option<Vec<u8>> {
    let gpu = Gpu::shared().filter(|gpu| gpu.owns(image))?;
    image.settle();
    gpu.read(&image.texture, rect).inspect_err(|e| log::warn!("render: {e}")).ok()
}

/// Writes into `image`; an image of a replaced device is left alone.
pub fn write(image: &GpuImage, data: &[u8], size: (u32, u32), from: [u32; 2], to: [u32; 4]) {
    if let Some(gpu) = Gpu::shared().filter(|gpu| gpu.owns(image)) {
        gpu.write(image, data, size, from, to);
    }
}

pub fn shadow(scene: Scene, size: (u32, u32), place: Affine, sigma: f64, color: [f32; 4]) -> Option<Arc<GpuImage>> {
    drawing(|o| o.shadow(scene, size, place, sigma, color))
}

pub fn filtered(scene: Scene, size: (u32, u32), place: Affine, filter: &crate::gpu::Filter) -> Option<Arc<GpuImage>> {
    drawing(|o| o.filtered(scene, size, place, filter))
}

pub fn set_cpu_raster(raster: CpuRaster) -> bool {
    CPU_RASTER.set(raster).is_ok()
}

pub fn cpu_raster_registered() -> bool {
    CPU_RASTER.get().is_some()
}

pub fn cpu_raster(scene: Scene, size: (u32, u32)) -> Option<Vec<u8>> {
    let raster = CPU_RASTER.get()?;
    raster(scene, size).filter(|pixels| whole(pixels, size))
}

fn whole(pixels: &[u8], (width, height): (u32, u32)) -> bool {
    pixels.len() as u64 == u64::from(width) * u64::from(height) * 4
}

pub fn rasterize(scene: Scene, size: (u32, u32)) -> Vec<u8> {
    drawing(|o| Ok(o.rasterize(scene, size))).unwrap_or_else(|| vec![0; (size.0 * size.1 * 4) as usize])
}

#[cfg(test)]
mod tests {
    use super::{Held, held, whole};

    #[test]
    fn a_live_device_is_used_without_asking_the_registry() {
        assert_eq!(held(false, || panic!("the registry is not asked")), Held::Use);
    }

    #[test]
    fn a_lost_device_waits_until_a_new_one_is_published() {
        assert_eq!(held(true, || true), Held::Wait);
        assert_eq!(held(true, || false), Held::Rebuild);
    }

    #[test]
    fn a_cpu_raster_answer_counts_only_with_four_bytes_for_every_pixel() {
        assert!(whole(&[0; 2 * 3 * 4], (2, 3)));
        assert!(!whole(&[0; 2 * 3 * 4 - 1], (2, 3)));
        assert!(!whole(&[], (2, 3)));
    }
}
