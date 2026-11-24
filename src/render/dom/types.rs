#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeType {
    Element = 1,
    Text = 3,
    CDataSection = 4,
    ProcessingInstruction = 7,
    Comment = 8,
    Document = 9,
    DocumentType = 10,
    DocumentFragment = 11,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ns {
    Html = 0,
    Svg = 1,
    MathMl = 2,
    Other = 3,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnimationEvent {
    pub kind: &'static str,
    pub transition: bool,
    pub target: u32,
    pub name: String,
    pub elapsed: f64,
    pub pseudo: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnimationEvents {
    pub animating: bool,
    pub events: Vec<AnimationEvent>,
}

#[derive(Clone, Debug, Default)]
pub struct FrameTake {
    pub scrolled: Vec<u32>,
    pub loaded: Vec<u32>,
    pub animation: Option<AnimationEvents>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub width: f64,
    pub height: f64,
    pub scale: f64,
}

pub type Quad = [f64; 4];
