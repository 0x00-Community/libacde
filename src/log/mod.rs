use std::io::Write;
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

const DEFAULT: &str =
    "warn,acde=info,acde_log=info,acde_napi=info,acde_render=info,acde_runtime=info,acde_host=info,js=info,usvg=error,tracing::span=off";
const LIMIT: usize = 8 << 20;

static START: OnceLock<Instant> = OnceLock::new();
static SINK: Sink = Sink { queue: Mutex::new(Queue { bytes: Vec::new(), dropped: 0, writing: false }), ready: Condvar::new(), idle: Condvar::new() };

struct Sink {
    queue: Mutex<Queue>,
    ready: Condvar,
    idle: Condvar,
}

struct Queue {
    bytes: Vec<u8>,
    dropped: u64,
    writing: bool,
}

pub fn start() -> Instant {
    *START.get_or_init(Instant::now)
}

pub fn now_ms() -> f64 {
    start().elapsed().as_secs_f64() * 1000.0
}

pub fn since_ms(at: Instant) -> f64 {
    at.elapsed().as_secs_f64() * 1000.0
}

pub fn init() {
    start();
    let mut builder = env_logger::Builder::new();
    builder.parse_filters(DEFAULT);
    if let Ok(spec) = std::env::var("RUST_LOG") {
        builder.parse_filters(&spec);
    }
    builder.format(format).target(env_logger::Target::Pipe(Box::new(Pipe))).write_style(env_logger::WriteStyle::Never);
    if builder.try_init().is_err() {
        return;
    }
    if let Err(e) = std::thread::Builder::new().name("acde-log".into()).spawn(drain) {
        log::set_max_level(log::LevelFilter::Off);
        eprintln!("libacde: no log thread ({e}), logging is off");
        return;
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        flush();
        previous(info);
    }));
}

fn format(buf: &mut env_logger::fmt::Formatter, record: &log::Record) -> std::io::Result<()> {
    let level = match record.level() {
        log::Level::Error => 'E',
        log::Level::Warn => 'W',
        log::Level::Info => 'I',
        log::Level::Debug => 'D',
        log::Level::Trace => 'T',
    };
    let thread = std::thread::current();
    writeln!(buf, "{:.3} {level} {} {}: {}", now_ms(), thread.name().unwrap_or("-"), record.target(), record.args())
}

struct Pipe;

impl Write for Pipe {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut queue = SINK.queue.lock().unwrap_or_else(|e| e.into_inner());
        if queue.bytes.len() + buf.len() > LIMIT {
            queue.dropped += 1;
            return Ok(buf.len());
        }
        let was_empty = queue.bytes.is_empty();
        queue.bytes.extend_from_slice(buf);
        drop(queue);
        if was_empty {
            SINK.ready.notify_one();
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn drain() {
    let mut spare = Vec::new();
    let mut stderr = std::io::stderr();
    loop {
        let mut queue = SINK.queue.lock().unwrap_or_else(|e| e.into_inner());
        while queue.bytes.is_empty() && queue.dropped == 0 {
            queue = SINK.ready.wait(queue).unwrap_or_else(|e| e.into_inner());
        }
        std::mem::swap(&mut queue.bytes, &mut spare);
        let dropped = std::mem::take(&mut queue.dropped);
        queue.writing = true;
        drop(queue);
        _ = stderr.write_all(&spare);
        if dropped > 0 {
            _ = writeln!(stderr, "{:.3} W acde-log acde_log: dropped {dropped} lines, stderr is too slow", now_ms());
        }
        spare.clear();
        SINK.queue.lock().unwrap_or_else(|e| e.into_inner()).writing = false;
        SINK.idle.notify_all();
    }
}

pub fn flush() {
    let deadline = Instant::now() + Duration::from_secs(1);
    let mut queue = SINK.queue.lock().unwrap_or_else(|e| e.into_inner());
    while !queue.bytes.is_empty() || queue.writing {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return;
        }
        queue = SINK.idle.wait_timeout(queue, left).unwrap_or_else(|e| e.into_inner()).0;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Trace {
    pub kind: &'static str,
    pub input: f64,
    pub pumped: f64,
    pub committed: f64,
    pub framed: f64,
    pub rastered: f64,
}

impl Trace {
    pub fn input(kind: &'static str, at: f64) -> Self {
        Self { kind, input: at, pumped: at, committed: at, framed: at, rastered: at }
    }

    pub fn earliest(a: Option<Self>, b: Option<Self>) -> Option<Self> {
        match (a, b) {
            (Some(a), Some(b)) => Some(if b.input < a.input { b } else { a }),
            (a, b) => a.or(b),
        }
    }

    pub fn presented(&self, owner: u32) {
        let now = now_ms();
        log::debug!(
            target: "perf::latency",
            "{} owner={owner} ms={:.3} queue={:.3} js={:.3} render={:.3} raster={:.3} present={:.3}",
            self.kind,
            now - self.input,
            self.pumped - self.input,
            self.committed - self.pumped,
            self.framed - self.committed,
            self.rastered - self.framed,
            now - self.rastered,
        );
    }
}
