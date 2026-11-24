use std::collections::HashMap;

pub use crate::model::{Event, Waker};
pub use crate::render::DragImage;
use crate::render::{RenderHandle, SurfaceCommand as C};

pub struct Surface(Box<dyn crate::host::platform::Window>);

pub struct PanelOpening {
    pub id: u32,
    pub token: u64,
    pub rect: [i32; 4],
    pub exclusive: bool,
    pub user_only: bool,
    pub popup: bool,
    pub parent: Option<u32>,
    pub kind: u32,
}

impl PanelOpening {
    pub const DOCK: u32 = 0;
    pub const FLOATING: u32 = 1;
    pub const NOTICE: u32 = 2;
    pub const CONFIRM: u32 = 3;
}

macro_rules! commands {
    ($($name:ident($($arg:ident: $t:ty),*) => $command:expr;)*) => {$(
        pub fn $name(&self, $($arg: $t),*) {
            self.0.send($command)
        }
    )*};
}

impl Surface {
    pub fn claim(token: u64, render: RenderHandle, frame: u32, app: Option<String>) -> Result<Self, String> {
        crate::host::rt::current().platform().claim(token, render, frame, app).map(Surface)
    }

    pub fn minimizable(&self) -> bool {
        self.0.minimizable()
    }

    pub fn host_controls(&self) -> bool {
        self.0.host_controls()
    }

    pub fn size(&self) -> Option<(u32, u32, f64)> {
        self.0.size()
    }

    pub fn hosted(&self) -> bool {
        self.0.hosted()
    }

    pub fn pointer_lock(&self) -> bool {
        self.0.pointer_lock()
    }

    pub fn window(&self) -> u32 {
        self.0.window()
    }

    pub fn content_offset(&self) -> (f64, f64) {
        self.0.content_offset()
    }

    pub fn open_panel(&self, panel: PanelOpening) -> bool {
        let PanelOpening { id, token, rect, exclusive, user_only, popup, parent, kind } = panel;
        self.0.open_panel(C::OpenPanel { id, token, rect, exclusive, user_only, popup, parent, kind })
    }

    pub fn ime_float(&self) -> bool {
        self.0.ime_float()
    }

    pub fn pump(&self) -> Vec<Event> {
        self.0.pump()
    }

    pub fn has_events(&self) -> bool {
        self.0.has_events()
    }

    pub fn set_waker(&self, waker: Option<Waker>) {
        self.0.set_waker(waker)
    }

    commands! {
        request_frame() => C::RequestFrame;
        set_clipboard(entries: Vec<crate::render::ClipboardEntry>) => C::SetClipboard(entries);
        clipboard_types(reply: crate::render::ClipboardTypesReply) => C::ClipboardTypes(reply);
        read_clipboard(mime: String, reply: crate::render::ClipboardReadReply) => C::ReadClipboard { mime, reply };
        start_drag(data: HashMap<String, String>, icon: Option<DragImage>) => C::StartDrag(data, icon);
        request_mode(mode: u32) => C::Mode(mode);
        request_state(state: u32) => C::State(state);
        set_grab_hint(rect: [i32; 4], edges: u32) => C::GrabHint(rect, edges);
        set_backdrop(rect: [i32; 4], effect: [f64; 5]) => C::Backdrop(rect, effect);
        request_activate() => C::Activate;
        request_grab(kind: u32, on: bool) => C::Grab(kind, on);
        lock_pointer(on: bool) => C::PointerLock(on);
        place_panel(id: u32, rect: [i32; 4], input: [i32; 4]) => C::PlacePanel { id, rect, input };
        close_panel(id: u32) => C::ClosePanel(id);
        reserve_panel(id: u32, edge: u32, thickness: f64) => C::ReservePanel { id, edge, thickness };
        set_double_click(action: u32) => C::DoubleClick(action);
        set_frame_theme(theme: crate::render::FrameTheme) => C::FrameTheme(theme);
        set_page_dark(dark: bool) => C::PageDark(dark);
        set_title(title: &str) => C::Title(title.to_owned());
        set_ime_area(rect: [f64; 4]) => C::ImeArea(rect);
        set_ime_float(float: Option<crate::render::ImeFloat>) => C::Ime(float);
        set_window_look(look: Option<crate::render::WindowLook>) => C::WindowLook(look);
        set_material(material: crate::render::WindowMaterial) => C::Material(material);
    }
}
