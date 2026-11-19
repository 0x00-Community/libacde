mod buffers;
pub mod consts;
mod context;
mod draw;
pub mod format;
mod framebuffer;
#[allow(dead_code, unused_imports, clippy::all, reason = "naga's GLSL frontend kept whole and in its own style: what WebGL does not take stays")]
mod glsl;
mod helpers;
mod live;
pub mod objects;
mod params;
mod pass;
mod pipeline;
mod program;
mod query;
mod read;
mod setters;
pub mod shader;
pub mod state;
mod texture;
mod vertex;

pub use context::{Attributes, Context, Limits};
pub use draw::ClearColor;
pub use objects::Id;
pub use params::Param;
pub use program::{ActiveInfo, Setter, Value};
pub use texture::TexSource;

pub(crate) type FastMap<K, V> = std::collections::HashMap<K, V, rustc_hash::FxBuildHasher>;
pub(crate) type FastSet<K> = std::collections::HashSet<K, rustc_hash::FxBuildHasher>;
