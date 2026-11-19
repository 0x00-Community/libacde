use std::collections::HashMap;

pub struct DragImage {
    pub width: u32,
    pub height: u32,
    pub scene: crate::gpu::Scene,
    pub hotspot: (i32, i32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardEntry {
    pub mime: String,
    pub data: Vec<u8>,
}

impl ClipboardEntry {
    pub const TEXT: &str = "text/plain";
    pub const PNG: &str = "image/png";
    pub const URI_LIST: &str = "text/uri-list";
    pub const TYPES: [&str; 3] = [Self::TEXT, Self::PNG, Self::URI_LIST];
    const WAYLAND_TEXT: [&str; 3] = ["text/plain;charset=utf-8", "text/plain", "UTF8_STRING"];

    pub fn new(mime: impl Into<String>, data: impl Into<Vec<u8>>) -> Self {
        Self { mime: mime.into(), data: data.into() }
    }

    pub fn text(text: impl Into<String>) -> Self {
        Self::new(Self::TEXT, text.into())
    }

    pub fn wayland_mimes(mime: &str) -> &'static [&'static str] {
        match mime {
            Self::TEXT => &Self::WAYLAND_TEXT,
            Self::PNG => &[Self::PNG],
            Self::URI_LIST => &[Self::URI_LIST],
            _ => &[],
        }
    }

    pub fn web_type(native: &str) -> Option<&'static str> {
        Self::TYPES.into_iter().find(|web| Self::wayland_mimes(web).contains(&native))
    }

    pub fn web_types<'a>(offered: impl IntoIterator<Item = &'a str>) -> Vec<String> {
        let mut types: Vec<String> = Vec::new();
        for web in offered.into_iter().filter_map(Self::web_type) {
            if !types.iter().any(|known| known == web) {
                types.push(web.to_owned());
            }
        }
        types
    }

    pub fn types(entries: &[ClipboardEntry]) -> Vec<String> {
        Self::web_types(entries.iter().map(|entry| entry.mime.as_str()))
    }

    pub fn find<'a>(entries: &'a [ClipboardEntry], mime: &str) -> Option<&'a ClipboardEntry> {
        entries.iter().find(|entry| entry.mime == mime)
    }
}

pub type ClipboardTypesReply = Box<dyn FnOnce(Vec<String>) + Send>;
pub type ClipboardReadReply = Box<dyn FnOnce(Option<Vec<u8>>) + Send>;

pub enum SurfaceCommand {
    RequestFrame,
    SetClipboard(Vec<ClipboardEntry>),
    ClipboardTypes(ClipboardTypesReply),
    ReadClipboard { mime: String, reply: ClipboardReadReply },
    StartDrag(HashMap<String, String>, Option<DragImage>),
    Mode(u32),
    State(u32),
    GrabHint([i32; 4], u32),
    Backdrop([i32; 4], [f64; 4]),
    Activate,
    Grab(u32, bool),
    PointerLock(bool),
    OpenPanel { id: u32, token: u64, rect: [i32; 4] },
    PlacePanel { id: u32, rect: [i32; 4], input: [i32; 4] },
    ClosePanel(u32),
    ReservePanel { id: u32, edge: u32, thickness: f64 },
    DoubleClick(u32),
    FrameTheme(crate::render::FrameTheme),
    PageDark(bool),
    Title(String),
    ImeArea([f64; 4]),
}

#[cfg(test)]
mod tests {
    use super::ClipboardEntry;

    #[test]
    fn wayland_types_become_web_types_in_order_without_unknown_or_repeated_ones() {
        let offered = ["image/png", "UTF8_STRING", "text/html", "text/plain;charset=utf-8", "text/uri-list"];
        assert_eq!(ClipboardEntry::web_types(offered), ["image/png", "text/plain", "text/uri-list"]);
        assert_eq!(ClipboardEntry::wayland_mimes("text/plain"), ["text/plain;charset=utf-8", "text/plain", "UTF8_STRING"]);
        assert!(ClipboardEntry::wayland_mimes("text/html").is_empty());
    }
}
