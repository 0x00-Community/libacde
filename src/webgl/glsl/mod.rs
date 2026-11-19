/*!
Frontend for [GLSL][glsl] (OpenGL Shading Language).

To begin, take a look at the documentation for the [`Frontend`].

# Supported versions
## Vulkan
- 440 (partial)
- 450
- 460

[glsl]: https://www.khronos.org/registry/OpenGL/index_gl.php
*/

pub use ast::{Precision, Profile};
pub use error::{Error, ErrorKind, ExpectedToken, ParseErrors};
pub use offset::PADDED;
pub use token::TokenValue;

use alloc::{string::String, vec::Vec};

use ast::{EntryArg, FunctionDeclaration, GlobalLookup};
use naga::{FastHashMap, FastHashSet, Handle, Module, ShaderStage, Span, Type, proc::Layouter};
use parser::ParsingContext;

mod ast;
mod builtins;
mod compat;
mod context;
mod error;
mod functions;
mod lex;
mod offset;
mod parser;
mod token;
mod types;
mod variables;

type Result<T> = core::result::Result<T, Error>;

/// Per-shader options passed to [`parse`](Frontend::parse).
///
/// The [`From`] trait is implemented for [`ShaderStage`] to provide a quick way
/// to create an `Options` instance.
///
/// ```rust
/// # use naga::ShaderStage;
/// # use naga::front::glsl::Options;
/// Options::from(ShaderStage::Vertex);
/// ```
#[derive(Debug)]
pub struct Options {
    /// The shader stage in the pipeline.
    pub stage: ShaderStage,
    /// Preprocessor definitions to be used, akin to having
    /// ```glsl
    /// #define key value
    /// ```
    /// for each key value pair in the map.
    pub defines: FastHashMap<String, String>,
}

impl From<ShaderStage> for Options {
    fn from(stage: ShaderStage) -> Self {
        Options { stage, defines: FastHashMap::default() }
    }
}

/// Additional information about the GLSL shader.
///
/// Stores additional information about the GLSL shader which might not be
/// stored in the shader [`Module`].
#[derive(Debug)]
pub struct ShaderMetadata {
    /// The GLSL version specified in the shader through the use of the
    /// `#version` preprocessor directive.
    pub version: u16,
    /// The GLSL profile specified in the shader through the use of the
    /// `#version` preprocessor directive.
    pub profile: Profile,
    /// The shader stage in the pipeline, passed to the [`parse`](Frontend::parse)
    /// method via the [`Options`] struct.
    pub stage: ShaderStage,

    /// The workgroup size for compute shaders, defaults to `[1; 3]` for
    /// compute shaders and `[0; 3]` for non compute shaders.
    pub workgroup_size: [u32; 3],
    /// Whether or not early fragment tests where requested by the shader.
    /// Defaults to `false`.
    pub early_fragment_tests: bool,

    /// The shader can request extensions via the
    /// `#extension` preprocessor directive, in the directive a behavior
    /// parameter is used to control whether the extension should be disabled,
    /// warn on usage, enabled if possible or required.
    ///
    /// This field only stores extensions which were required or requested to
    /// be enabled if possible and they are supported.
    pub extensions: FastHashSet<String>,
}

impl ShaderMetadata {
    fn reset(&mut self, stage: ShaderStage) {
        self.version = 0;
        self.profile = Profile::Core;
        self.stage = stage;
        self.workgroup_size = [u32::from(stage.compute_like()); 3];
        self.early_fragment_tests = false;
        self.extensions.clear();
    }
}

impl Default for ShaderMetadata {
    fn default() -> Self {
        ShaderMetadata {
            version: 0,
            profile: Profile::Core,
            stage: ShaderStage::Vertex,
            workgroup_size: [0; 3],
            early_fragment_tests: false,
            extensions: FastHashSet::default(),
        }
    }
}

/// The `Frontend` is the central structure of the GLSL frontend.
///
/// To instantiate a new `Frontend` the [`Default`] trait is used, so a
/// call to the associated function [`Frontend::default`](Frontend::default) will
/// return a new `Frontend` instance.
///
/// To parse a shader simply call the [`parse`](Frontend::parse) method with a
/// [`Options`] struct and a [`&str`](str) holding the glsl code.
///
/// The `Frontend` also provides the [`metadata`](Frontend::metadata) to get some
/// further information about the previously parsed shader, like version and
/// extensions used (see the documentation for
/// [`ShaderMetadata`] to see all the returned information)
///
/// # Example usage
/// ```rust
/// use naga::ShaderStage;
/// use naga::front::glsl::{Frontend, Options};
///
/// let glsl = r#"
///     #version 450 core
///
///     void main() {}
/// "#;
///
/// let mut frontend = Frontend::default();
/// let options = Options::from(ShaderStage::Vertex);
/// frontend.parse(&options, glsl);
/// ```
///
/// # Reusability
///
/// If there's a need to parse more than one shader reusing the same `Frontend`
/// instance may be beneficial since internal allocations will be reused.
///
/// Calling the [`parse`](Frontend::parse) method multiple times will reset the
/// `Frontend` so no extra care is needed when reusing.
// acde: WebGL (GLSL ES 1.00 / 3.00) dialect, see `Frontend::parse_webgl`
/// Where the WebGL dialect puts a shader's interface and resources.
#[derive(Debug, Default, Clone)]
pub struct WebGlOptions {
    /// Locations of inputs and outputs by name; unnamed ones get the next free location.
    pub locations: FastHashMap<String, u32>,
    /// Binding (in group 0) of the default uniform block.
    pub uniform_binding: u32,
    /// Group of the samplers, textures and uniform blocks.
    pub group: u32,
    /// First binding in `group`.
    pub first_binding: u32,
    /// Length of `gl_FragData`.
    pub draw_buffers: u32,
    /// Inputs and outputs that become plain globals (an output nothing reads, an input nothing writes).
    pub dropped: FastHashSet<String>,
    /// Points wider than a pixel: a vertex shader draws each point as a quad (six vertices of an
    /// instance) of `gl_PointSize` pixels, sized by the hidden uniform `webgl_point` (the viewport
    /// in `xy`), and passes the point coordinate at this location; a fragment shader reads
    /// `gl_PointCoord` from it.
    pub points: Option<u32>,
    /// `gl_MaxVaryingVectors` (and the vertex output / fragment input vectors): the context's limit.
    pub varyings: u32,
}

/// A shader input or output.
#[derive(Debug, Clone)]
pub struct WebGlInterface {
    pub name: String,
    pub handle: Handle<naga::GlobalVariable>,
    pub ty: Handle<Type>,
    pub location: u32,
    pub explicit: bool,
}

/// A sampler uniform (or array of them): an image and a sampler global per element.
#[derive(Debug, Clone)]
pub struct WebGlSampler {
    pub name: String,
    pub ty: Handle<Type>,
    pub array: Option<u32>,
    pub globals: Vec<(Handle<naga::GlobalVariable>, Handle<naga::GlobalVariable>)>,
    pub bindings: Vec<(u32, u32)>,
}

/// A uniform block.
#[derive(Debug, Clone)]
pub struct WebGlBlock {
    pub name: String,
    pub instance: Option<String>,
    pub handle: Handle<naga::GlobalVariable>,
    pub binding: u32,
}

/// What the WebGL dialect found in a shader.
#[derive(Debug, Default, Clone)]
pub struct WebGlShader {
    pub version: u16,
    pub inputs: Vec<WebGlInterface>,
    pub outputs: Vec<WebGlInterface>,
    /// The default uniform block; its members are the loose uniforms (bools stored as uint).
    pub uniforms: Option<Handle<naga::GlobalVariable>>,
    /// Declared type of each default block member.
    pub uniform_types: Vec<Handle<Type>>,
    pub samplers: Vec<WebGlSampler>,
    pub blocks: Vec<WebGlBlock>,
    pub frag_data: Option<Handle<naga::GlobalVariable>>,
    pub point_size: Option<Handle<naga::GlobalVariable>>,
}

#[derive(Debug, Default)]
struct WebGlState {
    options: WebGlOptions,
    shader: WebGlShader,
    next_input: u32,
    next_output: u32,
    next_binding: u32,
    members: Vec<naga::StructMember>,
    span: u32,
    block: bool,
    point: Option<(Handle<naga::GlobalVariable>, u32)>,
}

#[derive(Debug, Default)]
pub struct Frontend {
    meta: ShaderMetadata,

    lookup_function: FastHashMap<String, FunctionDeclaration>,
    lookup_type: FastHashMap<String, Handle<Type>>,

    global_variables: Vec<(String, GlobalLookup)>,

    entry_args: Vec<EntryArg>,

    layouter: Layouter,

    errors: Vec<Error>,

    // acde: WebGL dialect
    webgl: Option<WebGlState>,
}

impl Frontend {
    fn reset(&mut self, stage: ShaderStage) {
        self.meta.reset(stage);

        self.lookup_function.clear();
        self.lookup_type.clear();
        self.global_variables.clear();
        self.entry_args.clear();
        self.layouter.clear();
        self.webgl = None;
    }

    /// Parses a shader either outputting a shader [`Module`] or a list of
    /// [`Error`]s.
    ///
    /// Multiple calls using the same `Frontend` and different shaders are supported.
    pub fn parse(&mut self, options: &Options, source: &str) -> core::result::Result<Module, ParseErrors> {
        self.reset(options.stage);

        let lexer = lex::Lexer::new(source, &options.defines);
        let mut ctx = ParsingContext::new(lexer);

        match ctx.parse(self) {
            Ok(module) => {
                if self.errors.is_empty() {
                    Ok(module)
                } else {
                    Err(core::mem::take(&mut self.errors).into())
                }
            }
            Err(e) => {
                self.errors.push(e);
                Err(core::mem::take(&mut self.errors).into())
            }
        }
    }

    /// Returns additional information about the parsed shader which might not
    /// be stored in the [`Module`], see the documentation for
    /// [`ShaderMetadata`] for more information about the returned data.
    ///
    /// # Notes
    ///
    /// Following an unsuccessful parsing the state of the returned information
    /// is undefined, it might contain only partial information about the
    /// current shader, the previous shader or both.
    // acde: WebGL dialect
    /// Parses a WebGL shader (GLSL ES 1.00, or 3.00 with `#version 300 es`).
    ///
    /// Attributes, varyings and fragment outputs get locations from `webgl.locations` (else in order),
    /// loose uniforms go into one uniform buffer (group 0, `webgl.uniform_binding`), and each combined
    /// sampler becomes an image and a sampler (group `webgl.group`, from `webgl.first_binding`).
    /// The vertex position is flipped in y and its depth mapped to [0, 1].
    pub fn parse_webgl(&mut self, options: &Options, webgl: WebGlOptions, source: &str) -> core::result::Result<(Module, WebGlShader), ParseErrors> {
        self.reset(options.stage);
        self.meta.version = 100;
        self.meta.profile = Profile::Es;
        self.webgl = Some(WebGlState { next_binding: webgl.first_binding, options: webgl, ..Default::default() });

        let lexer = lex::Lexer::new(source, &options.defines).webgl();
        let mut ctx = ParsingContext::new(lexer);

        let result = ctx.parse(self);
        let mut state = self.webgl.take().unwrap_or_default();
        state.shader.version = self.meta.version;
        match result {
            Ok(module) if self.errors.is_empty() => Ok((module, state.shader)),
            Ok(_) => Err(core::mem::take(&mut self.errors).into()),
            Err(e) => {
                self.errors.push(e);
                Err(core::mem::take(&mut self.errors).into())
            }
        }
    }

    pub const fn metadata(&self) -> &ShaderMetadata {
        &self.meta
    }
}
