#[cfg(target_os = "linux")]
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, mpsc};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use pipewire as pw;
#[cfg(target_os = "linux")]
use pw::spa;

use super::mixer::{Cmd, Mixer, RATE};
use crate::host::rt::Msg;

pub const IDLE: Duration = Duration::from_secs(3);

pub enum Ctl {
    Open { web: u64, name: String, notify: mpsc::Sender<Msg> },
    Cmd(u64, Cmd),
    Close(u64),
    Idle(u64),
}

enum Sender {
    #[cfg(target_os = "linux")]
    Pw(pw::channel::Sender<Ctl>),
    Std(mpsc::Sender<Ctl>),
}

static OUTPUT: OnceLock<Option<Sender>> = OnceLock::new();

pub fn send(ctl: Ctl) -> bool {
    match OUTPUT.get_or_init(start) {
        #[cfg(target_os = "linux")]
        Some(Sender::Pw(tx)) => tx.send(ctl).is_ok(),
        Some(Sender::Std(tx)) => tx.send(ctl).is_ok(),
        None => false,
    }
}

fn start() -> Option<Sender> {
    let (tx, rx) = mpsc::sync_channel(1);
    let spawned = std::thread::Builder::new().name("acde-audio".into()).spawn(move || {
        if std::env::var("ACDE_AUDIO").is_ok_and(|v| v == "null") {
            log::info!("ACDE_AUDIO=null, sound plays nowhere");
        } else {
            #[cfg(target_os = "linux")]
            match pw_run(&tx) {
                Ok(()) => return,
                Err(e) => log::warn!("no PipeWire ({e}): sound plays nowhere"),
            }
            #[cfg(not(target_os = "linux"))]
            match super::output_cpal::Device::open() {
                Ok(device) => {
                    let (stx, srx) = mpsc::channel();
                    _ = tx.send(Sender::Std(stx.clone()));
                    device.run(stx, srx);
                    return;
                }
                Err(e) => log::warn!("no sound output ({e}): sound plays nowhere"),
            }
        }
        let (stx, srx) = mpsc::channel();
        _ = tx.send(Sender::Std(stx));
        null_loop(srx);
    });
    if let Err(e) = spawned {
        log::error!("{e}");
        return None;
    }
    rx.recv_timeout(Duration::from_secs(5)).ok()
}

pub struct Web<O> {
    pub mixer: Arc<Mutex<Mixer>>,
    pub notify: mpsc::Sender<Msg>,
    pub name: String,
    out: Option<O>,
}

pub fn lock(mixer: &Mutex<Mixer>) -> MutexGuard<'_, Mixer> {
    mixer.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn deliver(mixer: &mut Mixer, notify: &mpsc::Sender<Msg>) {
    let notes = mixer.take_notes();
    if !notes.is_empty() {
        _ = notify.send(Msg::Task(Box::new(move |scope| super::deliver(scope, notes))));
    }
}

pub fn apply(mixer: &mut Mixer, rendering: bool, cmd: Cmd) -> bool {
    if !rendering {
        mixer.catch_up();
    }
    let restart = match &cmd {
        Cmd::CreateContext { ctx, .. } | Cmd::Running { ctx, running: true } => Some(*ctx),
        _ => None,
    };
    mixer.apply(cmd);
    if let Some(ctx) = restart {
        mixer.restart_clock(ctx, rendering);
    }
    !rendering && mixer.active()
}

pub fn control<O>(webs: &mut HashMap<u64, Web<O>>, ctl: Ctl, open: impl FnOnce(u64, &Web<O>) -> Result<O, String>) {
    match ctl {
        Ctl::Open { web, name, notify } => {
            webs.insert(web, Web { mixer: Arc::default(), notify, name, out: None });
        }
        Ctl::Cmd(id, cmd) => {
            let Some(web) = webs.get_mut(&id) else { return };
            let start = apply(&mut lock(&web.mixer), web.out.is_some(), cmd);
            if start {
                match open(id, web) {
                    Ok(out) => {
                        lock(&web.mixer).set_rendering(true);
                        web.out = Some(out);
                    }
                    Err(e) => log::warn!("[{}] no sound stream: {e}", web.name),
                }
            }
        }
        Ctl::Close(id) => {
            webs.remove(&id);
        }
        Ctl::Idle(id) => {
            if let Some(web) = webs.get_mut(&id)
                && web.out.is_some()
                && !lock(&web.mixer).active()
            {
                web.out = None;
                lock(&web.mixer).set_rendering(false);
            }
        }
    }
}

pub struct Idle {
    web: u64,
    frames: u64,
    sent: bool,
}

impl Idle {
    pub fn new(web: u64) -> Idle {
        Idle { web, frames: 0, sent: false }
    }

    pub fn tick(&mut self, active: bool, frames: usize, send: impl FnOnce(Ctl) -> bool) {
        if active {
            self.frames = 0;
            self.sent = false;
        } else {
            self.frames += frames as u64;
        }
        if !self.sent && self.frames as f64 >= IDLE.as_secs_f64() * RATE {
            self.sent = send(Ctl::Idle(self.web));
        }
    }
}

#[cfg(target_os = "linux")]
struct PwStream {
    _listener: pw::stream::StreamListener<Process>,
    _stream: pw::stream::StreamRc,
}

#[cfg(target_os = "linux")]
struct Process {
    mixer: Arc<Mutex<Mixer>>,
    notify: mpsc::Sender<Msg>,
    ctl: pw::channel::Sender<Ctl>,
    scratch: Vec<f32>,
    idle: Idle,
    missed: usize,
}

#[cfg(target_os = "linux")]
impl Process {
    fn process(&mut self, stream: &pw::stream::Stream) {
        let Some(mut buffer) = stream.dequeue_buffer() else { return };
        let requested = buffer.requested() as usize;
        let Some(data) = buffer.datas_mut().first_mut() else { return };
        let mut frames = 0;
        if let Some(bytes) = data.data() {
            let max = bytes.len() / 8;
            frames = if requested > 0 { requested.min(max) } else { max.min(1024) };
            self.scratch.resize(frames * 2, 0.0);
            match self.mixer.try_lock() {
                Ok(mut mixer) => {
                    mixer.missed(std::mem::take(&mut self.missed));
                    mixer.render(&mut self.scratch);
                    deliver(&mut mixer, &self.notify);
                    let active = mixer.active();
                    drop(mixer);
                    self.idle.tick(active, frames, |ctl| self.ctl.send(ctl).is_ok());
                }
                Err(_) => {
                    self.scratch.fill(0.0);
                    self.missed += frames;
                }
            }
            for (out, v) in bytes[..frames * 8].as_chunks_mut::<4>().0.iter_mut().zip(&self.scratch) {
                out.copy_from_slice(&v.to_le_bytes());
            }
        }
        let chunk = data.chunk_mut();
        *chunk.offset_mut() = 0;
        *chunk.stride_mut() = 8;
        *chunk.size_mut() = (frames * 8) as u32;
    }
}

#[cfg(target_os = "linux")]
fn pw_run(ready: &mpsc::SyncSender<Sender>) -> Result<(), pw::Error> {
    pw::init();
    let main = pw::main_loop::MainLoopRc::new(None)?;
    let context = crate::runtime::pw_context(&main)?;
    let core = context.connect_rc(None)?;
    let (tx, rx) = pw::channel::channel();
    _ = ready.send(Sender::Pw(tx.clone()));
    let webs: RefCell<HashMap<u64, Web<PwStream>>> = RefCell::default();
    let _errors = core.add_listener_local().error(|id, _seq, res, message| log::warn!("PipeWire error {res} (object {id}): {message}")).register();
    let stream_core = core.clone();
    let _rx = rx.attach(main.loop_(), move |ctl| {
        control(&mut webs.borrow_mut(), ctl, |id, web| pw_stream(&stream_core, &tx, id, web).map_err(|e| e.to_string()));
    });
    main.run();
    Ok(())
}

#[cfg(target_os = "linux")]
fn pw_stream(core: &pw::core::CoreRc, ctl: &pw::channel::Sender<Ctl>, id: u64, web: &Web<PwStream>) -> Result<PwStream, pw::Error> {
    let props = pw::properties::properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Playback",
        *pw::keys::APP_NAME => "acde",
        *pw::keys::NODE_NAME => format!("acde.{}", web.name),
        *pw::keys::MEDIA_NAME => web.name.as_str(),
        *pw::keys::NODE_LATENCY => format!("512/{}", RATE as u32),
    };
    let stream = pw::stream::StreamRc::new(core.clone(), &web.name, props)?;
    let process = Process { mixer: web.mixer.clone(), notify: web.notify.clone(), ctl: ctl.clone(), scratch: Vec::new(), idle: Idle::new(id), missed: 0 };
    let listener = stream.add_local_listener_with_user_data(process).process(|stream, p| p.process(stream)).register()?;

    let mut info = spa::param::audio::AudioInfoRaw::new();
    info.set_format(spa::param::audio::AudioFormat::F32LE);
    info.set_rate(RATE as u32);
    info.set_channels(2);
    let mut position = [0; spa::param::audio::MAX_CHANNELS];
    position[0] = spa::sys::SPA_AUDIO_CHANNEL_FL;
    position[1] = spa::sys::SPA_AUDIO_CHANNEL_FR;
    info.set_position(position);
    let object =
        spa::pod::Value::Object(spa::pod::Object { type_: spa::sys::SPA_TYPE_OBJECT_Format, id: spa::sys::SPA_PARAM_EnumFormat, properties: info.into() });
    let bytes = spa::pod::serialize::PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &object).map_err(|_| pw::Error::CreationFailed)?.0.into_inner();
    let pod = spa::pod::Pod::from_bytes(&bytes).ok_or(pw::Error::CreationFailed)?;
    stream.connect(spa::utils::Direction::Output, None, pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS, &mut [pod])?;
    Ok(PwStream { _listener: listener, _stream: stream })
}

struct NullOut {
    start: Instant,
    rendered: u64,
    idle: u64,
}

fn null_loop(rx: mpsc::Receiver<Ctl>) {
    let mut webs: HashMap<u64, Web<NullOut>> = HashMap::new();
    let mut scratch = vec![0.0f32; 2 * 1024];
    loop {
        let rendering = webs.values().any(|w| w.out.is_some());
        let ctl = if rendering { rx.recv_timeout(Duration::from_millis(10)) } else { rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected) };
        match ctl {
            Ok(ctl) => control(&mut webs, ctl, |_, web| {
                log::debug!("[{}] sound renders into nothing", web.name);
                Ok(NullOut { start: Instant::now(), rendered: 0, idle: 0 })
            }),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        for web in webs.values_mut() {
            let Some(out) = &mut web.out else { continue };
            let mut mixer = lock(&web.mixer);
            let due = (out.start.elapsed().as_secs_f64() * RATE) as u64;
            while out.rendered < due {
                let frames = (due - out.rendered).min(1024) as usize;
                mixer.render(&mut scratch[..frames * 2]);
                out.rendered += frames as u64;
                if mixer.active() {
                    out.idle = 0;
                } else {
                    out.idle += frames as u64;
                }
            }
            deliver(&mut mixer, &web.notify);
            if out.idle as f64 >= IDLE.as_secs_f64() * RATE {
                mixer.set_rendering(false);
                drop(mixer);
                web.out = None;
            }
        }
    }
}
