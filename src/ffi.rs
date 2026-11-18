use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr::{null, null_mut};
use std::sync::Arc;

use crate::host::{API_LEVEL, Entry, Headless, Platform, Portal, Shared, Window};
use crate::model::{Event, FileFilter, OpenRequest, SaveRequest, Waker};
use crate::render::{RenderHandle, SurfaceCommand};

#[derive(Clone, Copy)]
struct Text(*const c_char);

impl From<Text> for Option<String> {
    fn from(Text(text): Text) -> Self {
        (!text.is_null()).then(|| unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned())
    }
}

#[derive(Clone, Copy)]
struct Texts(*const *const c_char, usize);

impl From<Texts> for Option<Vec<String>> {
    fn from(Texts(texts, len): Texts) -> Self {
        (!texts.is_null()).then(|| unsafe { std::slice::from_raw_parts(texts, len) }.iter().filter_map(|&text| Text(text).into()).collect())
    }
}

#[derive(Default)]
struct Held {
    texts: Vec<CString>,
    lists: Vec<Vec<*const c_char>>,
}

impl Held {
    fn text(&mut self, text: &str) -> *const c_char {
        let text = CString::new(text).unwrap_or_else(|_| CString::new(text.replace('\0', "")).unwrap_or_default());
        let ptr = text.as_ptr();
        self.texts.push(text);
        ptr
    }

    fn list<'a>(&mut self, texts: impl IntoIterator<Item = &'a String>) -> (*const *const c_char, usize) {
        let list: Vec<*const c_char> = texts.into_iter().map(|text| self.text(text)).collect();
        let out = (list.as_ptr(), list.len());
        self.lists.push(list);
        out
    }
}

pub struct AcdertStrings(Vec<String>);

impl AcdertStrings {
    fn first(self) -> Option<String> {
        self.0.into_iter().next()
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcdertChoice {
    Chosen,
    Cancelled,
    Failed,
}

impl AcdertChoice {
    fn many(self, strings: AcdertStrings) -> Result<Option<Vec<String>>, String> {
        match self {
            Self::Chosen => Ok(Some(strings.0)),
            Self::Cancelled => Ok(None),
            Self::Failed => Err(strings.first().unwrap_or_default()),
        }
    }

    fn one(self, strings: AcdertStrings) -> Result<Option<String>, String> {
        self.many(strings).map(|chosen| chosen.and_then(|paths| paths.into_iter().next()))
    }
}

#[repr(C)]
pub struct AcdertFilter {
    pub name: *const c_char,
    pub globs: *const *const c_char,
    pub globs_len: usize,
    pub mimes: *const *const c_char,
    pub mimes_len: usize,
}

#[repr(C)]
pub struct AcdertFileRequest {
    pub title: *const c_char,
    pub name: *const c_char,
    pub multiple: bool,
    pub directory: bool,
    pub filters: *const AcdertFilter,
    pub filters_len: usize,
}

struct Request {
    _held: Held,
    _filters: Vec<AcdertFilter>,
    request: AcdertFileRequest,
}

impl Request {
    fn new(title: &str, name: Option<&str>, multiple: bool, directory: bool, filters: &[FileFilter]) -> Self {
        let mut held = Held::default();
        let filters: Vec<AcdertFilter> = filters
            .iter()
            .map(|filter| {
                let (globs, globs_len) = held.list(&filter.globs);
                let (mimes, mimes_len) = held.list(&filter.mimes);
                AcdertFilter { name: held.text(&filter.name), globs, globs_len, mimes, mimes_len }
            })
            .collect();
        let request = AcdertFileRequest {
            title: held.text(title),
            name: name.map_or(null(), |name| held.text(name)),
            multiple,
            directory,
            filters: filters.as_ptr(),
            filters_len: filters.len(),
        };
        Self { _held: held, _filters: filters, request }
    }
}

impl From<&OpenRequest> for Request {
    fn from(request: &OpenRequest) -> Self {
        Self::new(&request.title, None, request.multiple, request.directory, &request.filters)
    }
}

impl From<&SaveRequest> for Request {
    fn from(request: &SaveRequest) -> Self {
        Self::new(&request.title, Some(&request.name), false, false, &request.filters)
    }
}

#[repr(C)]
pub struct AcdertEvent {
    pub kind: *const c_char,
    pub window: i64,
    pub parent: i64,
    pub button: i64,
    pub code: i64,
    pub x: f64,
    pub y: f64,
    pub delta_x: f64,
    pub delta_y: f64,
    pub width: f64,
    pub height: f64,
    pub time: f64,
    pub scale: f64,
    pub stamp: f64,
    pub key: *const c_char,
    pub title: *const c_char,
    pub app_id: *const c_char,
    pub state: *const c_char,
    pub previous: *const c_char,
    pub mime: *const *const c_char,
    pub mime_len: usize,
    pub files: *const *const c_char,
    pub files_len: usize,
    pub data_keys: *const *const c_char,
    pub data_values: *const *const c_char,
    pub data_len: usize,
}

impl Default for AcdertEvent {
    fn default() -> Self {
        Self {
            kind: null(),
            window: -1,
            parent: -1,
            button: -1,
            code: -1,
            x: f64::NAN,
            y: f64::NAN,
            delta_x: f64::NAN,
            delta_y: f64::NAN,
            width: f64::NAN,
            height: f64::NAN,
            time: f64::NAN,
            scale: f64::NAN,
            stamp: f64::NAN,
            key: null(),
            title: null(),
            app_id: null(),
            state: null(),
            previous: null(),
            mime: null(),
            mime_len: 0,
            files: null(),
            files_len: 0,
            data_keys: null(),
            data_values: null(),
            data_len: 0,
        }
    }
}

impl From<&AcdertEvent> for Event {
    fn from(event: &AcdertEvent) -> Self {
        let float = |value: f64| (!value.is_nan()).then_some(value);
        let count = |value: i64| u32::try_from(value).ok();
        let keys: Option<Vec<String>> = Texts(event.data_keys, event.data_len).into();
        let values: Option<Vec<String>> = Texts(event.data_values, event.data_len).into();
        Event {
            kind: Option::from(Text(event.kind)).unwrap_or_default(),
            window: count(event.window),
            parent: count(event.parent),
            button: count(event.button),
            code: count(event.code),
            x: float(event.x),
            y: float(event.y),
            delta_x: float(event.delta_x),
            delta_y: float(event.delta_y),
            width: float(event.width),
            height: float(event.height),
            time: float(event.time),
            scale: float(event.scale),
            stamp: float(event.stamp),
            key: Text(event.key).into(),
            title: Text(event.title).into(),
            app_id: Text(event.app_id).into(),
            state: Text(event.state).into(),
            previous: Text(event.previous).into(),
            mime: Texts(event.mime, event.mime_len).into(),
            files: Texts(event.files, event.files_len).into(),
            data: keys.zip(values).map(|(keys, values)| keys.into_iter().zip(values).collect()),
            instance: None,
        }
    }
}

pub struct AcdertEvents(Vec<Event>);

pub struct AcdertWaker(Waker);

pub struct AcdertRender(RenderHandle);

pub struct AcdertReply(Box<dyn FnOnce(String) + Send>);

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcdertCommandKind {
    RequestFrame,
    SetClipboard,
    ReadClipboard,
    StartDrag,
    Mode,
    State,
    GrabHint,
    Backdrop,
    Activate,
    Grab,
    OpenPanel,
    PlacePanel,
    ClosePanel,
    ReservePanel,
    DoubleClick,
    FrameTheme,
    PageDark,
    Title,
    ImeArea,
}

#[repr(C)]
pub struct AcdertCommand {
    pub kind: AcdertCommandKind,
    pub text: *const c_char,
    pub number: u32,
    pub id: u32,
    pub token: u64,
    pub flag: bool,
    pub rect: [i32; 4],
    pub input: [i32; 4],
    pub values: [f64; 4],
    pub thickness: f64,
    pub data_keys: *const *const c_char,
    pub data_values: *const *const c_char,
    pub data_len: usize,
    pub reply: *mut AcdertReply,
}

impl From<AcdertCommandKind> for AcdertCommand {
    fn from(kind: AcdertCommandKind) -> Self {
        Self {
            kind,
            text: null(),
            number: 0,
            id: 0,
            token: 0,
            flag: false,
            rect: [0; 4],
            input: [0; 4],
            values: [0.0; 4],
            thickness: 0.0,
            data_keys: null(),
            data_values: null(),
            data_len: 0,
            reply: null_mut(),
        }
    }
}

struct Outgoing {
    _held: Held,
    command: AcdertCommand,
}

impl From<SurfaceCommand> for Outgoing {
    fn from(command: SurfaceCommand) -> Self {
        use AcdertCommandKind as Kind;
        let mut held = Held::default();
        let command = match command {
            SurfaceCommand::RequestFrame => Kind::RequestFrame.into(),
            SurfaceCommand::SetClipboard(text) => AcdertCommand { text: held.text(&text), ..Kind::SetClipboard.into() },
            SurfaceCommand::ReadClipboard(reply) => AcdertCommand { reply: Box::into_raw(Box::new(AcdertReply(reply))), ..Kind::ReadClipboard.into() },
            SurfaceCommand::StartDrag(data, _) => {
                let (data_keys, data_len) = held.list(data.keys());
                let (data_values, _) = held.list(data.values());
                AcdertCommand { data_keys, data_values, data_len, ..Kind::StartDrag.into() }
            }
            SurfaceCommand::Mode(number) => AcdertCommand { number, ..Kind::Mode.into() },
            SurfaceCommand::State(number) => AcdertCommand { number, ..Kind::State.into() },
            SurfaceCommand::GrabHint(rect, number) => AcdertCommand { rect, number, ..Kind::GrabHint.into() },
            SurfaceCommand::Backdrop(rect, values) => AcdertCommand { rect, values, ..Kind::Backdrop.into() },
            SurfaceCommand::Activate => Kind::Activate.into(),
            SurfaceCommand::Grab(number, flag) => AcdertCommand { number, flag, ..Kind::Grab.into() },
            SurfaceCommand::OpenPanel { id, token, rect } => AcdertCommand { id, token, rect, ..Kind::OpenPanel.into() },
            SurfaceCommand::PlacePanel { id, rect, input } => AcdertCommand { id, rect, input, ..Kind::PlacePanel.into() },
            SurfaceCommand::ClosePanel(id) => AcdertCommand { id, ..Kind::ClosePanel.into() },
            SurfaceCommand::ReservePanel { id, edge, thickness } => AcdertCommand { id, number: edge, thickness, ..Kind::ReservePanel.into() },
            SurfaceCommand::DoubleClick(number) => AcdertCommand { number, ..Kind::DoubleClick.into() },
            SurfaceCommand::FrameTheme(_) => Kind::FrameTheme.into(),
            SurfaceCommand::PageDark(flag) => AcdertCommand { flag, ..Kind::PageDark.into() },
            SurfaceCommand::Title(text) => AcdertCommand { text: held.text(&text), ..Kind::Title.into() },
            SurfaceCommand::ImeArea(values) => AcdertCommand { values, ..Kind::ImeArea.into() },
        };
        Self { _held: held, command }
    }
}

#[repr(C)]
pub struct AcdertWindow {
    pub ctx: *mut c_void,
    pub window: Option<unsafe extern "C" fn(*mut c_void) -> u32>,
    pub content_offset: Option<unsafe extern "C" fn(*mut c_void, *mut f64, *mut f64)>,
    pub pump: Option<unsafe extern "C" fn(*mut c_void, *mut AcdertEvents)>,
    pub has_events: Option<unsafe extern "C" fn(*mut c_void) -> bool>,
    pub set_waker: Option<unsafe extern "C" fn(*mut c_void, *mut AcdertWaker)>,
    pub send: Option<unsafe extern "C" fn(*mut c_void, *const AcdertCommand)>,
    pub minimizable: Option<unsafe extern "C" fn(*mut c_void) -> bool>,
    pub host_controls: Option<unsafe extern "C" fn(*mut c_void) -> bool>,
    pub size: Option<unsafe extern "C" fn(*mut c_void, *mut u32, *mut u32, *mut f64) -> bool>,
    pub hosted: Option<unsafe extern "C" fn(*mut c_void) -> bool>,
    pub release: Option<unsafe extern "C" fn(*mut c_void)>,
}

impl Default for AcdertWindow {
    fn default() -> Self {
        Self {
            ctx: null_mut(),
            window: None,
            content_offset: None,
            pump: None,
            has_events: None,
            set_waker: None,
            send: None,
            minimizable: None,
            host_controls: None,
            size: None,
            hosted: None,
            release: None,
        }
    }
}

struct CWindow(AcdertWindow);

impl CWindow {
    fn ask(&self, call: Option<unsafe extern "C" fn(*mut c_void) -> bool>, otherwise: bool) -> bool {
        call.map_or(otherwise, |call| unsafe { call(self.0.ctx) })
    }
}

impl Window for CWindow {
    fn window(&self) -> u32 {
        self.0.window.map_or(0, |call| unsafe { call(self.0.ctx) })
    }

    fn content_offset(&self) -> (f64, f64) {
        let (mut x, mut y) = (0.0, 0.0);
        if let Some(call) = self.0.content_offset {
            unsafe { call(self.0.ctx, &mut x, &mut y) };
        }
        (x, y)
    }

    fn pump(&self) -> Vec<Event> {
        let mut events = AcdertEvents(Vec::new());
        if let Some(call) = self.0.pump {
            unsafe { call(self.0.ctx, &mut events) };
        }
        events.0
    }

    fn has_events(&self) -> bool {
        self.ask(self.0.has_events, false)
    }

    fn set_waker(&self, waker: Option<Waker>) {
        if let Some(call) = self.0.set_waker {
            let waker = waker.map_or(null_mut(), |waker| Box::into_raw(Box::new(AcdertWaker(waker))));
            unsafe { call(self.0.ctx, waker) };
        }
    }

    fn send(&self, command: SurfaceCommand) {
        if let Some(call) = self.0.send {
            let outgoing = Outgoing::from(command);
            unsafe { call(self.0.ctx, &outgoing.command) };
        }
    }

    fn minimizable(&self) -> bool {
        self.ask(self.0.minimizable, true)
    }

    fn host_controls(&self) -> bool {
        self.ask(self.0.host_controls, false)
    }

    fn size(&self) -> Option<(u32, u32, f64)> {
        let call = self.0.size?;
        let (mut width, mut height, mut scale) = (0, 0, 1.0);
        unsafe { call(self.0.ctx, &mut width, &mut height, &mut scale) }.then_some((width, height, scale))
    }

    fn hosted(&self) -> bool {
        self.ask(self.0.hosted, false)
    }
}

impl Drop for CWindow {
    fn drop(&mut self) {
        if let Some(release) = self.0.release {
            unsafe { release(self.0.ctx) };
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AcdertPlatform {
    pub ctx: *mut c_void,
    pub claim: Option<unsafe extern "C" fn(*mut c_void, u64, *mut AcdertRender, u32, *const c_char, *mut AcdertWindow, *mut AcdertStrings) -> bool>,
    pub content_type: Option<unsafe extern "C" fn(*mut c_void, *const c_char, *mut AcdertStrings)>,
    pub choose_folder: Option<unsafe extern "C" fn(*mut c_void, *const c_char, *mut AcdertStrings) -> AcdertChoice>,
    pub open_files: Option<unsafe extern "C" fn(*mut c_void, *const AcdertFileRequest, *mut AcdertStrings) -> AcdertChoice>,
    pub save_file: Option<unsafe extern "C" fn(*mut c_void, *const AcdertFileRequest, *mut AcdertStrings) -> AcdertChoice>,
    pub release: Option<unsafe extern "C" fn(*mut c_void)>,
}

struct CPlatform(AcdertPlatform);

unsafe impl Send for CPlatform {}
unsafe impl Sync for CPlatform {}

impl CPlatform {
    fn choose(
        &self,
        call: unsafe extern "C" fn(*mut c_void, *const AcdertFileRequest, *mut AcdertStrings) -> AcdertChoice,
        request: Request,
    ) -> (AcdertChoice, AcdertStrings) {
        let mut strings = AcdertStrings(Vec::new());
        let choice = unsafe { call(self.0.ctx, &request.request, &mut strings) };
        (choice, strings)
    }
}

impl Platform for CPlatform {
    fn claim(&self, token: u64, render: RenderHandle, frame: u32, app: Option<String>) -> Result<Box<dyn Window>, String> {
        let Some(claim) = self.0.claim else {
            return Headless.claim(token, render, frame, app);
        };
        let mut held = Held::default();
        let app = app.as_deref().map_or(null(), |app| held.text(app));
        let render = Box::into_raw(Box::new(AcdertRender(render)));
        let (mut window, mut error) = (AcdertWindow::default(), AcdertStrings(Vec::new()));
        match unsafe { claim(self.0.ctx, token, render, frame, app, &mut window, &mut error) } {
            true => Ok(Box::new(CWindow(window))),
            false => Err(error.first().unwrap_or_else(|| format!("claimSurface {token}: refused"))),
        }
    }

    fn content_type(&self, extension: &str) -> Option<String> {
        let call = self.0.content_type?;
        let (mut held, mut strings) = (Held::default(), AcdertStrings(Vec::new()));
        unsafe { call(self.0.ctx, held.text(extension), &mut strings) };
        strings.first()
    }

    fn choose_folder(&self, title: &str) -> Result<Option<String>, String> {
        let Some(call) = self.0.choose_folder else {
            return Portal.choose_folder(title);
        };
        let (mut held, mut strings) = (Held::default(), AcdertStrings(Vec::new()));
        unsafe { call(self.0.ctx, held.text(title), &mut strings) }.one(strings)
    }

    fn open_files(&self, request: OpenRequest) -> Result<Option<Vec<String>>, String> {
        let Some(call) = self.0.open_files else {
            return Portal.open_files(request);
        };
        let (choice, strings) = self.choose(call, (&request).into());
        choice.many(strings)
    }

    fn save_file(&self, request: SaveRequest) -> Result<Option<String>, String> {
        let Some(call) = self.0.save_file else {
            return Portal.save_file(request);
        };
        let (choice, strings) = self.choose(call, (&request).into());
        choice.one(strings)
    }
}

impl Drop for CPlatform {
    fn drop(&mut self) {
        if let Some(release) = self.0.release {
            unsafe { release(self.0.ctx) };
        }
    }
}

struct Launch {
    args: Vec<String>,
    platform: Shared,
}

impl Launch {
    unsafe fn new(argc: usize, argv: *const *const c_char, platform: *const AcdertPlatform) -> Self {
        let args = Option::from(Texts(argv, argc)).unwrap_or_default();
        let platform: Shared = match unsafe { platform.as_ref() } {
            Some(platform) => Arc::new(CPlatform(*platform)),
            None => Arc::new(Headless),
        };
        Self { args, platform }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn acdert_api_level() -> u32 {
    API_LEVEL
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn acdert_version(program: *const c_char) -> *mut c_char {
    let program = Option::from(Text(program)).unwrap_or_else(|| String::from("acdert"));
    CString::new(crate::host::version(&program)).map_or(null_mut(), CString::into_raw)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn acdert_string_free(text: *mut c_char) {
    if !text.is_null() {
        drop(unsafe { CString::from_raw(text) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn acdert_start(script: *const c_char, argc: usize, argv: *const *const c_char, platform: *const AcdertPlatform) -> i32 {
    let Launch { args, platform } = unsafe { Launch::new(argc, argv, platform) };
    let entry = Option::from(Text(script)).map_or(Entry::Broker, Entry::Script);
    crate::host::start(entry, &args, platform)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn acdert_common(argc: usize, argv: *const *const c_char, platform: *const AcdertPlatform, code: *mut i32) -> bool {
    let Launch { args, platform } = unsafe { Launch::new(argc, argv, platform) };
    let Some(exit) = crate::host::common(&args, &platform) else {
        return false;
    };
    if let Some(code) = unsafe { code.as_mut() } {
        *code = exit;
    }
    true
}

#[unsafe(no_mangle)]
pub extern "C" fn acdert_exit(code: i32) -> ! {
    crate::host::exit(code)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn acdert_strings_push(strings: *mut AcdertStrings, text: *const c_char) {
    if let (Some(strings), Some(text)) = (unsafe { strings.as_mut() }, Option::from(Text(text))) {
        strings.0.push(text);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn acdert_event_init() -> AcdertEvent {
    AcdertEvent::default()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn acdert_events_push(events: *mut AcdertEvents, event: *const AcdertEvent) {
    if let (Some(events), Some(event)) = unsafe { (events.as_mut(), event.as_ref()) } {
        events.0.push(event.into());
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn acdert_waker_wake(waker: *const AcdertWaker) {
    if let Some(AcdertWaker(wake)) = unsafe { waker.as_ref() } {
        wake();
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn acdert_waker_free(waker: *mut AcdertWaker) {
    if !waker.is_null() {
        drop(unsafe { Box::from_raw(waker) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn acdert_reply_send(reply: *mut AcdertReply, text: *const c_char) {
    if !reply.is_null() {
        let AcdertReply(reply) = *unsafe { Box::from_raw(reply) };
        reply(Option::from(Text(text)).unwrap_or_default());
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn acdert_reply_free(reply: *mut AcdertReply) {
    if !reply.is_null() {
        drop(unsafe { Box::from_raw(reply) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn acdert_render_tick(render: *const AcdertRender, now_ms: f64) {
    if let Some(AcdertRender(render)) = unsafe { render.as_ref() } {
        render.tick(now_ms);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn acdert_render_pointer(render: *const AcdertRender, x: f32, y: f32) {
    if let Some(AcdertRender(render)) = unsafe { render.as_ref() } {
        render.pointer(x, y);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn acdert_render_scroll_at(render: *const AcdertRender, x: f32, y: f32, dx: f64, dy: f64) {
    if let Some(AcdertRender(render)) = unsafe { render.as_ref() } {
        render.scroll_at(x, y, dx, dy);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn acdert_render_commit(render: *const AcdertRender) -> u64 {
    unsafe { render.as_ref() }.map_or(0, |AcdertRender(render)| render.commit())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn acdert_render_invalidate(render: *const AcdertRender) {
    if let Some(AcdertRender(render)) = unsafe { render.as_ref() } {
        render.invalidate();
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn acdert_render_free(render: *mut AcdertRender) {
    if !render.is_null() {
        drop(unsafe { Box::from_raw(render) });
    }
}
