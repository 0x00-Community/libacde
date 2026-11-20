use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;

use blitz_dom::node::{ImageData, SpecialElementData, TextInputData};
use blitz_dom::{BaseDocument, Node, NodeData, NodeId, QualName, ns};
use style_dom::ElementState;

use crate::render::canvas::Canvas;
use crate::render::caret::{Caret, utf16_to_byte};
use crate::render::engine::Engine;
use crate::render::geometry::Geometry;
use crate::render::serialize::Serializer;
use crate::render::tree::{Doctype, DocumentInfo, Tree};
use crate::render::{Capture, CaretPosition, Error, Metrics, Result, Shared, Stats, TextMetrics};

mod dom_tree;
mod dom_view;
mod engine_state;
mod engine_tree;
mod select;
mod semantics;
mod types;
mod values;

pub use types::{AnimationEvent, AnimationEvents, NodeType, Ns, Quad, Viewport};

use values::{Control, Piece, RangeValue, collapse_whitespace, css_context, namespace, ns_of, number_string};

const INPUT_TTL_MS: f64 = 1000.0;

pub const NONE: u32 = u32::MAX;

const FRAGMENT: &str = "#document-fragment";

const LOADING: u8 = 0;
const LOADED: u8 = 1;
const FAILED: u8 = 2;
const NO_SRC: u8 = 3;

pub(crate) fn is_fragment(n: &Node) -> bool {
    n.element_data().is_some_and(|e| is_fragment_name(&e.name))
}

fn fail(name: &str, message: impl std::fmt::Display) -> Error {
    Error::new(format!("{name}: {message}"))
}

#[derive(Clone)]
pub struct Dom {
    pub(super) shared: Arc<Shared>,
    pub(crate) wake: Sender<()>,
}

impl Dom {
    fn read<T>(&self, f: impl FnOnce(&mut Engine) -> Result<T>) -> Result<T> {
        self.shared.script_call(&self.wake, f)
    }

    fn on<T>(&self, id: u32, f: impl FnOnce(&mut Engine, NodeId) -> T) -> Result<T> {
        self.read(|e| e.n(id).map(|node| f(e, node)))
    }

    fn write<T>(&self, f: impl FnOnce(&mut Engine) -> Result<T>) -> Result<T> {
        self.write_if(None, f, |_| true)
    }

    fn update(&self, f: impl FnOnce(&mut Engine)) -> Result<()> {
        self.write(|e| {
            f(e);
            Ok(())
        })
    }

    fn write_at<T>(&self, targets: &[u32], f: impl FnOnce(&mut Engine) -> Result<T>) -> Result<T> {
        self.write_if(Some(targets), f, |_| true)
    }

    fn edit<T>(&self, id: u32, f: impl FnOnce(&mut Engine, NodeId) -> T) -> Result<T> {
        self.write_at(&[id], |e| e.n(id).map(|node| f(e, node)))
    }

    fn create(&self, f: impl FnOnce(&mut Engine) -> Result<NodeId>) -> Result<u32> {
        self.write_at(&[], |e| {
            let node = f(e)?;
            Ok(e.fresh(node))
        })
    }

    fn write_changed(&self, targets: &[u32], f: impl FnOnce(&mut Engine) -> Result<bool>) -> Result<bool> {
        self.write_if(Some(targets), f, |r| !matches!(r, Ok(false)))
    }

    fn touched(&self) {
        self.shared.touched.store(true, Ordering::Relaxed);
    }

    fn write_if<T>(&self, targets: Option<&[u32]>, f: impl FnOnce(&mut Engine) -> Result<T>, changed: fn(&Result<T>) -> bool) -> Result<T> {
        self.touched();
        self.shared.script_call(&self.wake, |engine| {
            let marks = (engine.uncommitted, engine.stale, engine.dirty);
            let shown = |e: &Engine| targets.is_none_or(|t| t.iter().any(|&id| e.ids.node(id).is_ok_and(|n| e.connected(n))));
            let before = shown(engine);
            let result = f(engine);
            match changed(&result) {
                false => (engine.uncommitted, engine.stale, engine.dirty) = marks,
                true if before || shown(engine) => {
                    engine.uncommitted = true;
                    engine.touch();
                }
                true => {
                    engine.uncommitted = true;
                    (engine.stale, engine.dirty) = (marks.1, marks.2);
                }
            }
            result
        })
    }

    fn write_paint<T>(&self, id: u32, f: impl FnOnce(&mut Engine) -> Result<T>) -> Result<T> {
        self.touched();
        self.shared.script_call(&self.wake, |engine| {
            engine.uncommitted = true;
            let (stale, dirty) = (engine.stale, engine.dirty);
            let result = f(engine);
            engine.stale = stale;
            engine.dirty = dirty || engine.ids.node(id).is_ok_and(|n| engine.connected(n));
            result
        })
    }
}

fn bad_selector(selector: &str) -> Error {
    fail("SyntaxError", format!("'{selector}' is not a valid selector"))
}

fn shown(doc: &BaseDocument, node: NodeId) -> Option<Geometry<'_>> {
    let geometry = Geometry { doc };
    doc.get_node(node).filter(|n| n.is_element() && n.flags.is_in_document() && !geometry.hidden(n)).map(|_| geometry)
}

fn quad(r: crate::render::Rect) -> Quad {
    [r.x, r.y, r.width, r.height]
}

fn qualified(name: &QualName) -> String {
    match &name.prefix {
        Some(prefix) => format!("{prefix}:{}", name.local),
        None => name.local.to_string(),
    }
}

fn is_fragment_name(name: &QualName) -> bool {
    name.ns == ns!(html) && &*name.local == FRAGMENT
}

fn valid_element_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else { return false };
    match first.is_ascii_alphabetic() {
        true => !name.contains(|c: char| c.is_ascii_whitespace() || matches!(c, '\0' | '/' | '>')),
        false => {
            matches!(first, ':' | '_')
                || !first.is_ascii() && { chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | ':' | '_') || !c.is_ascii()) }
        }
    }
}

fn valid_attribute_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(|c: char| c.is_ascii_whitespace() || matches!(c, '\0' | '/' | '>' | '='))
}
