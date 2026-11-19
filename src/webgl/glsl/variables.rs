use alloc::{format, string::String, vec, vec::Vec};

use super::{
    Frontend, Result, Span,
    ast::*,
    context::{Context, ExprPos},
    error::{Error, ErrorKind},
};
use naga::{
    AddressSpace, Binding, BuiltIn, Constant, Expression, GlobalVariable, Handle, LocalVariable, Override, ResourceBinding, Scalar, ShaderStage,
    SwizzleComponent, Type, TypeInner, VectorSize,
};

pub struct VarDeclaration<'a, 'key> {
    pub qualifiers: &'a mut TypeQualifiers<'key>,
    pub ty: Handle<Type>,
    pub name: Option<String>,
    pub init: Option<Handle<Expression>>,
    pub meta: Span,
}

/// Information about a builtin used in [`add_builtin`](Frontend::add_builtin).
struct BuiltInData {
    /// The type of the builtin.
    inner: TypeInner,
    /// The associated builtin class.
    builtin: BuiltIn,
    /// Whether the builtin can be written to or not.
    mutable: bool,
    /// The storage used for the builtin.
    storage: StorageQualifier,
}

pub enum GlobalOrConstant {
    Global(Handle<GlobalVariable>),
    Constant(Handle<Constant>),
    Override(Handle<Override>),
}

impl Frontend {
    /// Adds a builtin and returns a variable reference to it
    fn add_builtin(&mut self, ctx: &mut Context, name: &str, data: BuiltInData, meta: Span) -> Result<Option<VariableReference>> {
        let ty = ctx.module.types.insert(Type { name: None, inner: data.inner }, meta);

        let handle = ctx.module.global_variables.append(
            GlobalVariable {
                name: Some(name.into()),
                space: AddressSpace::Private,
                binding: None,
                ty,
                init: None,
                memory_decorations: naga::MemoryDecorations::empty(),
            },
            meta,
        );

        let idx = self.entry_args.len();
        // acde: WebGL's int gl_VertexID / gl_InstanceID come from uint bindings
        let convert = match (data.builtin, &ctx.module.types[ty].inner) {
            (BuiltIn::VertexIndex | BuiltIn::InstanceIndex, &TypeInner::Scalar(Scalar::I32)) => Some(Scalar::U32),
            _ => None,
        };
        self.entry_args.push(EntryArg { name: Some(name.into()), binding: Binding::BuiltIn(data.builtin), handle, storage: data.storage, convert });

        self.global_variables.push((name.into(), GlobalLookup { kind: GlobalLookupKind::Variable(handle), entry_arg: Some(idx), mutable: data.mutable }));

        let expr = ctx.add_expression(Expression::GlobalVariable(handle), meta)?;

        let var = VariableReference { expr, load: true, mutable: data.mutable, constant: None, entry_arg: Some(idx) };

        ctx.symbol_table.add_root(name.into(), var.clone());

        Ok(Some(var))
    }

    pub(crate) fn lookup_variable(&mut self, ctx: &mut Context, name: &str, meta: Span) -> Result<Option<VariableReference>> {
        if let Some(var) = ctx.symbol_table.lookup(name).cloned() {
            return Ok(Some(var));
        }

        // acde: WebGL builtins
        if self.webgl.is_some() {
            if let Some(var) = self.webgl_builtin(ctx, name, meta)? {
                return Ok(Some(var));
            }
        }

        let data = match name {
            "gl_Position" => BuiltInData {
                inner: TypeInner::Vector { size: VectorSize::Quad, scalar: Scalar::F32 },
                builtin: BuiltIn::Position { invariant: false },
                mutable: true,
                storage: StorageQualifier::Output,
            },
            "gl_FragCoord" => BuiltInData {
                inner: TypeInner::Vector { size: VectorSize::Quad, scalar: Scalar::F32 },
                builtin: BuiltIn::Position { invariant: false },
                mutable: false,
                storage: StorageQualifier::Input,
            },
            "gl_PointCoord" => BuiltInData {
                inner: TypeInner::Vector { size: VectorSize::Bi, scalar: Scalar::F32 },
                builtin: BuiltIn::PointCoord,
                mutable: false,
                storage: StorageQualifier::Input,
            },
            "gl_GlobalInvocationID" | "gl_NumWorkGroups" | "gl_WorkGroupSize" | "gl_WorkGroupID" | "gl_LocalInvocationID" => BuiltInData {
                inner: TypeInner::Vector { size: VectorSize::Tri, scalar: Scalar::U32 },
                builtin: match name {
                    "gl_GlobalInvocationID" => BuiltIn::GlobalInvocationId,
                    "gl_NumWorkGroups" => BuiltIn::NumWorkGroups,
                    "gl_WorkGroupSize" => BuiltIn::WorkGroupSize,
                    "gl_WorkGroupID" => BuiltIn::WorkGroupId,
                    "gl_LocalInvocationID" => BuiltIn::LocalInvocationId,
                    _ => unreachable!(),
                },
                mutable: false,
                storage: StorageQualifier::Input,
            },
            "gl_FrontFacing" => {
                BuiltInData { inner: TypeInner::Scalar(Scalar::BOOL), builtin: BuiltIn::FrontFacing, mutable: false, storage: StorageQualifier::Input }
            }
            "gl_PointSize" | "gl_FragDepth" => BuiltInData {
                inner: TypeInner::Scalar(Scalar::F32),
                builtin: match name {
                    "gl_PointSize" => BuiltIn::PointSize,
                    "gl_FragDepth" => BuiltIn::FragDepth,
                    _ => unreachable!(),
                },
                mutable: true,
                storage: StorageQualifier::Output,
            },
            "gl_ClipDistance" | "gl_CullDistance" => {
                let base = ctx.module.types.insert(Type { name: None, inner: TypeInner::Scalar(Scalar::F32) }, meta);

                BuiltInData {
                    inner: TypeInner::Array { base, size: naga::ArraySize::Dynamic, stride: 4 },
                    builtin: match name {
                        "gl_ClipDistance" => BuiltIn::ClipDistances,
                        "gl_CullDistance" => BuiltIn::CullDistance,
                        _ => unreachable!(),
                    },
                    mutable: self.meta.stage == ShaderStage::Vertex,
                    storage: StorageQualifier::Output,
                }
            }
            _ => {
                let builtin = match name {
                    "gl_BaseVertex" => BuiltIn::BaseVertex,
                    "gl_BaseInstance" => BuiltIn::BaseInstance,
                    "gl_PrimitiveID" => BuiltIn::PrimitiveIndex,
                    "gl_BaryCoordEXT" => BuiltIn::Barycentric { perspective: true },
                    "gl_BaryCoordNoPerspEXT" => BuiltIn::Barycentric { perspective: false },
                    "gl_InstanceIndex" => BuiltIn::InstanceIndex,
                    "gl_VertexIndex" => BuiltIn::VertexIndex,
                    "gl_SampleID" => BuiltIn::SampleIndex,
                    "gl_LocalInvocationIndex" => BuiltIn::LocalInvocationIndex,
                    "gl_DrawID" => BuiltIn::DrawIndex,
                    _ => return Ok(None),
                };

                BuiltInData { inner: TypeInner::Scalar(Scalar::U32), builtin, mutable: false, storage: StorageQualifier::Input }
            }
        };

        self.add_builtin(ctx, name, data, meta)
    }

    pub(crate) fn make_variable_invariant(&mut self, ctx: &mut Context, name: &str, meta: Span) -> Result<()> {
        if let Some(var) = self.lookup_variable(ctx, name, meta)? {
            if let Some(index) = var.entry_arg {
                if let Binding::BuiltIn(BuiltIn::Position { ref mut invariant }) = self.entry_args[index].binding {
                    *invariant = true;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn field_selection(
        &mut self,
        ctx: &mut Context,
        pos: ExprPos,
        expression: Handle<Expression>,
        name: &str,
        meta: Span,
    ) -> Result<Handle<Expression>> {
        let (ty, is_pointer) = match *ctx.resolve_type(expression, meta)? {
            TypeInner::Pointer { base, .. } => (&ctx.module.types[base].inner, true),
            ref ty => (ty, false),
        };
        match *ty {
            TypeInner::Struct { ref members, .. } => {
                let index =
                    members.iter().position(|m| m.name == Some(name.into())).ok_or_else(|| Error { kind: ErrorKind::UnknownField(name.into()), meta })?;
                let pointer = ctx.add_expression(Expression::AccessIndex { base: expression, index: index as u32 }, meta)?;

                Ok(match pos {
                    ExprPos::Rhs if is_pointer => ctx.add_expression(Expression::Load { pointer }, meta)?,
                    _ => pointer,
                })
            }
            // swizzles (xyzw, rgba, stpq)
            TypeInner::Vector { size, .. } => {
                let check_swizzle_components = |comps: &str| {
                    name.chars()
                        .map(|c| comps.find(c).filter(|i| *i < size as usize).map(|i| SwizzleComponent::from_index(i as u32)))
                        .collect::<Option<Vec<SwizzleComponent>>>()
                };

                let components = check_swizzle_components("xyzw").or_else(|| check_swizzle_components("rgba")).or_else(|| check_swizzle_components("stpq"));

                if let Some(components) = components {
                    if let ExprPos::Lhs = pos {
                        let not_unique = (1..components.len()).any(|i| components[i..].contains(&components[i - 1]));
                        if not_unique {
                            self.errors.push(Error {
                                kind: ErrorKind::SemanticError(
                                    format!(concat!("swizzle cannot have duplicate components in ", "left-hand-side expression for \"{:?}\""), name).into(),
                                ),
                                meta,
                            })
                        }
                    }

                    let mut pattern = [SwizzleComponent::X; 4];
                    for (pat, component) in pattern.iter_mut().zip(&components) {
                        *pat = *component;
                    }

                    // flatten nested swizzles (vec.zyx.xy.x => vec.z)
                    let mut expression = expression;
                    if let Expression::Swizzle { size: _, vector, pattern: ref src_pattern } = ctx[expression] {
                        expression = vector;
                        for pat in &mut pattern {
                            *pat = src_pattern[pat.index() as usize];
                        }
                    }

                    let size = match components.len() {
                        // Swizzles with just one component are accesses and not swizzles
                        1 => {
                            match pos {
                                // If the position is in the right hand side and the base
                                // vector is a pointer, load it, otherwise the swizzle would
                                // produce a pointer
                                ExprPos::Rhs if is_pointer => {
                                    expression = ctx.add_expression(Expression::Load { pointer: expression }, meta)?;
                                }
                                _ => {}
                            };
                            return ctx.add_expression(Expression::AccessIndex { base: expression, index: pattern[0].index() }, meta);
                        }
                        2 => VectorSize::Bi,
                        3 => VectorSize::Tri,
                        4 => VectorSize::Quad,
                        _ => {
                            self.errors.push(Error { kind: ErrorKind::SemanticError(format!("Bad swizzle size for \"{name:?}\"").into()), meta });

                            VectorSize::Quad
                        }
                    };

                    if is_pointer {
                        // NOTE: for lhs expression, this extra load ends up as an unused expr, because the
                        // assignment will extract the pointer and use it directly anyway. Unfortunately we
                        // need it for validation to pass, as swizzles cannot operate on pointer values.
                        expression = ctx.add_expression(Expression::Load { pointer: expression }, meta)?;
                    }

                    Ok(ctx.add_expression(Expression::Swizzle { size, vector: expression, pattern }, meta)?)
                } else {
                    Err(Error { kind: ErrorKind::SemanticError(format!("Invalid swizzle for vector \"{name}\"").into()), meta })
                }
            }
            _ => Err(Error { kind: ErrorKind::SemanticError(format!("Can't lookup field on this type \"{name}\"").into()), meta }),
        }
    }

    pub(crate) fn add_global_var(
        &mut self,
        ctx: &mut Context,
        VarDeclaration { qualifiers, mut ty, name, init, meta }: VarDeclaration,
    ) -> Result<GlobalOrConstant> {
        let storage = qualifiers.storage.0;
        let (ret, lookup) = match storage {
            // acde: a dropped WebGL input or output is a plain global
            StorageQualifier::Input | StorageQualifier::Output
                if self.webgl.as_ref().is_some_and(|s| name.as_ref().is_some_and(|n| s.options.dropped.contains(n))) =>
            {
                qualifiers.interpolation.take();
                qualifiers.sampling.take();
                qualifiers.invariant.take();
                qualifiers.uint_layout_qualifier("location", &mut self.errors);
                let handle = ctx.module.global_variables.append(
                    GlobalVariable {
                        name: name.clone(),
                        space: AddressSpace::Private,
                        binding: None,
                        ty,
                        init,
                        memory_decorations: naga::MemoryDecorations::empty(),
                    },
                    meta,
                );
                let lookup = GlobalLookup { kind: GlobalLookupKind::Variable(handle), entry_arg: None, mutable: true };
                (GlobalOrConstant::Global(handle), lookup)
            }
            StorageQualifier::Input | StorageQualifier::Output => {
                let input = storage == StorageQualifier::Input;
                // TODO: glslang seems to use a counter for variables without
                // explicit location (even if that causes collisions)
                // acde: whether the location was written (WebGL)
                let explicit_location = qualifiers.uint_layout_qualifier("location", &mut self.errors);
                let location = explicit_location.unwrap_or(0);

                let interpolation = qualifiers.interpolation.take().map(|(i, _)| i);
                let sampling = qualifiers.sampling.take().map(|(s, _)| s);

                // acde: WebGL locations: given by name, else in order
                let location = match self.webgl.as_mut() {
                    Some(state) => {
                        let explicit = explicit_location.is_some();
                        let slots = interface_slots(&ctx.module.types, ty);
                        let given = name.as_ref().and_then(|n| state.options.locations.get(n).copied());
                        let next = match input {
                            true => &mut state.next_input,
                            false => &mut state.next_output,
                        };
                        let location = match (explicit, given) {
                            (true, _) => location,
                            (false, Some(given)) => given,
                            (false, None) => *next,
                        };
                        *next = (*next).max(location.saturating_add(slots));
                        location
                    }
                    None => location,
                };

                let handle = ctx.module.global_variables.append(
                    GlobalVariable {
                        name: name.clone(),
                        space: AddressSpace::Private,
                        binding: None,
                        ty,
                        init,
                        memory_decorations: naga::MemoryDecorations::empty(),
                    },
                    meta,
                );

                if let Some(state) = self.webgl.as_mut() {
                    let explicit = explicit_location.is_some();
                    let interface = crate::webgl::glsl::WebGlInterface { name: name.clone().unwrap_or_default(), handle, ty, location, explicit };
                    match input {
                        true => state.shader.inputs.push(interface),
                        false => state.shader.outputs.push(interface),
                    }
                }

                let blend_src = qualifiers.layout_qualifiers.remove(&QualifierKey::Index).and_then(|(value, _span)| match value {
                    QualifierValue::Uint(index) => Some(index),
                    _ => None,
                });

                let mut binding = Binding::Location { location, interpolation, sampling, blend_src, per_primitive: false };

                super::compat::apply_default_interpolation(&mut binding, &ctx.module.types[ty].inner);

                let idx = self.entry_args.len();
                self.entry_args.push(EntryArg { name: name.clone(), binding, handle, storage, convert: None });

                let lookup = GlobalLookup { kind: GlobalLookupKind::Variable(handle), entry_arg: Some(idx), mutable: !input };

                (GlobalOrConstant::Global(handle), lookup)
            }
            StorageQualifier::Const => {
                // Check if this is a specialization constant with constant_id
                let constant_id = qualifiers.uint_layout_qualifier("constant_id", &mut self.errors);

                if let Some(id) = constant_id {
                    // This is a specialization constant - convert to Override
                    let id: Option<u16> = match id.try_into() {
                        Ok(v) => Some(v),
                        Err(_) => {
                            self.errors.push(Error {
                                kind: ErrorKind::SemanticError(format!("constant_id value {id} is too high (maximum is {})", u16::MAX).into()),
                                meta,
                            });
                            None
                        }
                    };

                    let override_handle = ctx.module.overrides.append(Override { name: name.clone(), id, ty, init }, meta);

                    let lookup = GlobalLookup { kind: GlobalLookupKind::Override(override_handle, ty), entry_arg: None, mutable: false };

                    (GlobalOrConstant::Override(override_handle), lookup)
                } else {
                    // Regular constant
                    let init = init.ok_or_else(|| Error { kind: ErrorKind::SemanticError("const values must have an initializer".into()), meta })?;

                    let constant = Constant { name: name.clone(), ty, init };
                    let handle = ctx.module.constants.append(constant, meta);

                    let lookup = GlobalLookup { kind: GlobalLookupKind::Constant(handle, ty), entry_arg: None, mutable: false };

                    (GlobalOrConstant::Constant(handle), lookup)
                }
            }
            // acde: WebGL loose uniforms and samplers
            StorageQualifier::AddressSpace(AddressSpace::Uniform) if self.webgl.as_ref().is_some_and(|s| !s.block) => {
                let name = name.clone().unwrap_or_default();
                let kind = self.webgl_uniform(ctx, &name, ty, meta)?;
                let handle = match kind {
                    GlobalLookupKind::BlockSelect(h, _)
                    | GlobalLookupKind::UniformBool(h, _)
                    | GlobalLookupKind::Sampled(h, _)
                    | GlobalLookupKind::SampledArray(h, _) => h,
                    _ => unreachable!(),
                };
                let lookup = GlobalLookup { kind, entry_arg: None, mutable: false };
                (GlobalOrConstant::Global(handle), lookup)
            }
            StorageQualifier::AddressSpace(mut space) => {
                match space {
                    AddressSpace::Storage { ref mut access } => {
                        if let Some((allowed_access, _)) = qualifiers.storage_access.take() {
                            *access = allowed_access;
                        }
                    }
                    AddressSpace::Uniform => match ctx.module.types[ty].inner {
                        TypeInner::Image { class, dim, arrayed } => {
                            if let naga::ImageClass::Storage { mut access, mut format } = class {
                                if let Some((allowed_access, _)) = qualifiers.storage_access.take() {
                                    access = allowed_access;
                                }

                                match qualifiers.layout_qualifiers.remove(&QualifierKey::Format) {
                                    Some((QualifierValue::Format(f), _)) => format = f,
                                    // TODO: glsl supports images without format qualifier
                                    // if they are `writeonly`
                                    None => {
                                        self.errors.push(Error { kind: ErrorKind::SemanticError("image types require a format layout qualifier".into()), meta })
                                    }
                                    _ => unreachable!(),
                                }

                                ty = ctx.module.types.insert(
                                    Type { name: None, inner: TypeInner::Image { dim, arrayed, class: naga::ImageClass::Storage { format, access } } },
                                    meta,
                                );
                            }

                            space = AddressSpace::Handle
                        }
                        TypeInner::Sampler { .. } => space = AddressSpace::Handle,
                        _ => {
                            if qualifiers.none_layout_qualifier("push_constant", &mut self.errors) {
                                space = AddressSpace::Immediate
                            }
                        }
                    },
                    AddressSpace::Function => space = AddressSpace::Private,
                    _ => {}
                };

                let binding = match space {
                    // acde: WebGL uniform blocks take the next binding
                    AddressSpace::Uniform if self.webgl.is_some() => {
                        let state = self.webgl.as_mut().unwrap();
                        let binding = state.next_binding;
                        state.next_binding += 1;
                        Some(ResourceBinding { group: state.options.group, binding })
                    }
                    AddressSpace::Uniform | AddressSpace::Storage { .. } | AddressSpace::Handle => {
                        let binding = qualifiers.uint_layout_qualifier("binding", &mut self.errors);
                        if binding.is_none() {
                            self.errors.push(Error { kind: ErrorKind::SemanticError("uniform/buffer blocks require layout(binding=X)".into()), meta });
                        }
                        let set = qualifiers.uint_layout_qualifier("set", &mut self.errors);
                        binding.map(|binding| ResourceBinding { group: set.unwrap_or(0), binding })
                    }
                    _ => None,
                };

                let handle = ctx
                    .module
                    .global_variables
                    .append(GlobalVariable { name: name.clone(), space, binding, ty, init, memory_decorations: naga::MemoryDecorations::empty() }, meta);

                let lookup = GlobalLookup { kind: GlobalLookupKind::Variable(handle), entry_arg: None, mutable: true };

                (GlobalOrConstant::Global(handle), lookup)
            }
        };

        if let Some(name) = name {
            ctx.add_global(&name, lookup)?;

            self.global_variables.push((name, lookup));
        }

        qualifiers.unused_errors(&mut self.errors);

        Ok(ret)
    }

    pub(crate) fn add_local_var(&mut self, ctx: &mut Context, decl: VarDeclaration) -> Result<Handle<Expression>> {
        let storage = decl.qualifiers.storage;
        let mutable = match storage.0 {
            StorageQualifier::AddressSpace(AddressSpace::Function) => true,
            StorageQualifier::Const => false,
            _ => {
                self.errors.push(Error { kind: ErrorKind::SemanticError("Locals cannot have a storage qualifier".into()), meta: storage.1 });
                true
            }
        };

        let handle = ctx.locals.append(LocalVariable { name: decl.name.clone(), ty: decl.ty, init: decl.init }, decl.meta);
        let expr = ctx.add_expression(Expression::LocalVariable(handle), decl.meta)?;

        if let Some(name) = decl.name {
            let maybe_var = ctx.add_local_var(name.clone(), expr, mutable);

            if maybe_var.is_some() {
                self.errors.push(Error { kind: ErrorKind::VariableAlreadyDeclared(name), meta: decl.meta })
            }
        }

        decl.qualifiers.unused_errors(&mut self.errors);

        Ok(expr)
    }
}

// acde: WebGL dialect
const MAX_SAMPLER_ARRAY: u32 = 32;

// acde: WebGL dialect
fn interface_slots(types: &naga::UniqueArena<Type>, ty: Handle<Type>) -> u32 {
    match types[ty].inner {
        TypeInner::Matrix { columns, .. } => columns as u32,
        TypeInner::Array { base, size: naga::ArraySize::Constant(size), .. } => size.get().saturating_mul(interface_slots(types, base)),
        TypeInner::Struct { ref members, .. } => members.iter().fold(0u32, |n, m| n.saturating_add(interface_slots(types, m.ty))),
        _ => 1,
    }
}

// acde: WebGL dialect
fn contains_bool(types: &naga::UniqueArena<Type>, ty: Handle<Type>) -> bool {
    match types[ty].inner {
        TypeInner::Scalar(scalar) | TypeInner::Vector { scalar, .. } | TypeInner::Matrix { scalar, .. } => scalar.kind == naga::ScalarKind::Bool,
        TypeInner::Array { base, .. } => contains_bool(types, base),
        TypeInner::Struct { ref members, .. } => members.iter().any(|m| contains_bool(types, m.ty)),
        _ => false,
    }
}

// acde: WebGL dialect
impl Frontend {
    pub(crate) fn webgl_uniform(&mut self, ctx: &mut Context, name: &str, ty: Handle<Type>, meta: Span) -> Result<GlobalLookupKind> {
        let (base, array) = match ctx.module.types[ty].inner {
            TypeInner::Array { base, size: naga::ArraySize::Constant(size), .. } if matches!(ctx.module.types[base].inner, TypeInner::Image { .. }) => {
                (base, Some(size.get()))
            }
            _ => (ty, None),
        };
        // acde: a sampler array has a pair of globals per element; WebGL has at most 32 texture units
        if array.is_some_and(|n| n > MAX_SAMPLER_ARRAY) {
            return Err(Error { kind: ErrorKind::SemanticError(format!("sampler array '{name}' is longer than {MAX_SAMPLER_ARRAY}").into()), meta });
        }
        if let TypeInner::Image { class, .. } = ctx.module.types[base].inner {
            let comparison = matches!(class, naga::ImageClass::Depth { .. });
            let sampler_ty = ctx.module.types.insert(Type { name: None, inner: TypeInner::Sampler { comparison } }, meta);
            let Some(state) = self.webgl.as_mut() else { unreachable!() };
            let group = state.options.group;
            let mut globals = Vec::new();
            let mut bindings = Vec::new();
            for i in 0..array.unwrap_or(1) {
                let binding = state.next_binding;
                state.next_binding += 2;
                let element = match array {
                    Some(_) => format!("{name}_{i}"),
                    None => name.into(),
                };
                let image = ctx.module.global_variables.append(
                    GlobalVariable {
                        name: Some(element.clone()),
                        space: AddressSpace::Handle,
                        binding: Some(ResourceBinding { group, binding }),
                        ty: base,
                        init: None,
                        memory_decorations: naga::MemoryDecorations::empty(),
                    },
                    meta,
                );
                let sampler = ctx.module.global_variables.append(
                    GlobalVariable {
                        name: Some(format!("{element}_sampler")),
                        space: AddressSpace::Handle,
                        binding: Some(ResourceBinding { group, binding: binding + 1 }),
                        ty: sampler_ty,
                        init: None,
                        memory_decorations: naga::MemoryDecorations::empty(),
                    },
                    meta,
                );
                globals.push((image, sampler));
                bindings.push((binding, binding + 1));
            }
            let (image, sampler) = globals[0];
            state.shader.samplers.push(crate::webgl::glsl::WebGlSampler { name: name.into(), ty: base, array, globals, bindings });
            return Ok(match array {
                Some(len) => GlobalLookupKind::SampledArray(image, len),
                None => GlobalLookupKind::Sampled(image, sampler),
            });
        }

        let (stored, boolean) = match ctx.module.types[ty].inner {
            TypeInner::Scalar(Scalar::BOOL) => (ctx.module.types.insert(Type { name: None, inner: TypeInner::Scalar(Scalar::U32) }, meta), true),
            TypeInner::Vector { size, scalar: Scalar::BOOL } => {
                (ctx.module.types.insert(Type { name: None, inner: TypeInner::Vector { size, scalar: Scalar::U32 } }, meta), true)
            }
            _ => (ty, false),
        };
        if !boolean && contains_bool(&ctx.module.types, ty) {
            self.errors.push(Error { kind: ErrorKind::SemanticError(format!("uniform '{name}': arrays and structs of bool are not supported").into()), meta });
        }
        let info = super::offset::calculate_offset(stored, meta, StructLayout::WebGl, &mut ctx.module.types, &mut self.errors);
        let Some(state) = self.webgl.as_mut() else { unreachable!() };
        let offset = info.align.round_up(state.span);
        state.span = offset + info.span;
        state.members.push(naga::StructMember { name: Some(name.into()), ty: info.ty, binding: None, offset });
        state.shader.uniform_types.push(ty);
        let span = naga::proc::Alignment::MIN_UNIFORM.round_up(state.span);
        let struct_ty =
            ctx.module.types.insert(Type { name: Some("acde_uniforms".into()), inner: TypeInner::Struct { members: state.members.clone(), span } }, meta);
        let handle = match state.shader.uniforms {
            Some(handle) => {
                ctx.module.global_variables.get_mut(handle).ty = struct_ty;
                handle
            }
            None => {
                let handle = ctx.module.global_variables.append(
                    GlobalVariable {
                        name: Some("acde_uniforms".into()),
                        space: AddressSpace::Uniform,
                        binding: Some(ResourceBinding { group: 0, binding: state.options.uniform_binding }),
                        ty: struct_ty,
                        init: None,
                        memory_decorations: naga::MemoryDecorations::empty(),
                    },
                    meta,
                );
                state.shader.uniforms = Some(handle);
                handle
            }
        };
        let index = state.members.len() as u32 - 1;
        Ok(match boolean {
            true => GlobalLookupKind::UniformBool(handle, index),
            false => GlobalLookupKind::BlockSelect(handle, index),
        })
    }

    fn webgl_global(
        &mut self,
        ctx: &mut Context,
        name: &str,
        ty: Handle<Type>,
        meta: Span,
        entry_arg: Option<Binding>,
    ) -> Result<(Handle<GlobalVariable>, VariableReference)> {
        let handle = ctx.module.global_variables.append(
            GlobalVariable {
                name: Some(name.into()),
                space: AddressSpace::Private,
                binding: None,
                ty,
                init: None,
                memory_decorations: naga::MemoryDecorations::empty(),
            },
            meta,
        );
        let entry_arg = entry_arg.map(|binding| {
            self.entry_args.push(EntryArg { name: Some(name.into()), binding, handle, storage: StorageQualifier::Output, convert: None });
            self.entry_args.len() - 1
        });
        let expr = ctx.add_expression(Expression::GlobalVariable(handle), meta)?;
        let var = VariableReference { expr, load: true, mutable: true, constant: None, entry_arg };
        ctx.symbol_table.add_root(name.into(), var.clone());
        self.global_variables.push((name.into(), GlobalLookup { kind: GlobalLookupKind::Variable(handle), entry_arg, mutable: true }));
        Ok((handle, var))
    }

    fn webgl_builtin(&mut self, ctx: &mut Context, name: &str, meta: Span) -> Result<Option<VariableReference>> {
        let fragment = self.meta.stage == ShaderStage::Fragment;
        let vec4 = TypeInner::Vector { size: VectorSize::Quad, scalar: Scalar::F32 };
        let limit = match name {
            "gl_MaxVertexAttribs" => Some(16),
            "gl_MaxVertexUniformVectors" => Some(1024),
            "gl_MaxFragmentUniformVectors" => Some(1024),
            "gl_MaxVaryingVectors" | "gl_MaxVertexOutputVectors" | "gl_MaxFragmentInputVectors" => {
                Some(self.webgl.as_ref().map_or(15, |s| i32::try_from(s.options.varyings).unwrap_or(i32::MAX)))
            }
            "gl_MaxVertexTextureImageUnits" | "gl_MaxTextureImageUnits" => Some(16),
            "gl_MaxCombinedTextureImageUnits" => Some(32),
            "gl_MaxDrawBuffers" => Some(8),
            "gl_MinProgramTexelOffset" => Some(-8),
            "gl_MaxProgramTexelOffset" => Some(7),
            _ => None,
        };
        if let Some(value) = limit {
            let expr = ctx.add_expression(Expression::Literal(naga::Literal::I32(value)), meta)?;
            return Ok(Some(VariableReference { expr, load: false, mutable: false, constant: None, entry_arg: None }));
        }
        // acde: wide points pass the point coordinate in from the vertex shader
        if name == "gl_PointCoord" && fragment {
            if let Some(location) = self.webgl.as_ref().and_then(|s| s.options.points) {
                let ty = ctx.module.types.insert(Type { name: None, inner: TypeInner::Vector { size: VectorSize::Bi, scalar: Scalar::F32 } }, meta);
                let handle = ctx.module.global_variables.append(
                    GlobalVariable {
                        name: Some(name.into()),
                        space: AddressSpace::Private,
                        binding: None,
                        ty,
                        init: None,
                        memory_decorations: naga::MemoryDecorations::empty(),
                    },
                    meta,
                );
                self.entry_args.push(EntryArg {
                    name: Some(name.into()),
                    binding: Binding::Location {
                        location,
                        interpolation: Some(naga::Interpolation::Perspective),
                        sampling: Some(naga::Sampling::Center),
                        blend_src: None,
                        per_primitive: false,
                    },
                    handle,
                    storage: StorageQualifier::Input,
                    convert: None,
                });
                let idx = self.entry_args.len() - 1;
                let expr = ctx.add_expression(Expression::GlobalVariable(handle), meta)?;
                let var = VariableReference { expr, load: true, mutable: false, constant: None, entry_arg: Some(idx) };
                ctx.symbol_table.add_root(name.into(), var.clone());
                self.global_variables.push((name.into(), GlobalLookup { kind: GlobalLookupKind::Variable(handle), entry_arg: Some(idx), mutable: false }));
                return Ok(Some(var));
            }
        }
        // acde: otherwise points are one pixel, so gl_PointCoord is the centre
        if name == "gl_PointCoord" && fragment {
            let half = ctx.add_expression(Expression::Literal(naga::Literal::F32(0.5)), meta)?;
            let ty = ctx.module.types.insert(Type { name: None, inner: TypeInner::Vector { size: VectorSize::Bi, scalar: Scalar::F32 } }, meta);
            let expr = ctx.add_expression(Expression::Compose { ty, components: vec![half, half] }, meta)?;
            return Ok(Some(VariableReference { expr, load: false, mutable: false, constant: None, entry_arg: None }));
        }
        let output = |location| Binding::Location { location, interpolation: None, sampling: None, blend_src: None, per_primitive: false };
        let data = |inner, builtin, mutable, storage| BuiltInData { inner, builtin, mutable, storage };
        match name {
            "gl_FragColor" if fragment => {
                let ty = ctx.module.types.insert(Type { name: None, inner: vec4 }, meta);
                let (handle, var) = self.webgl_global(ctx, name, ty, meta, Some(output(0)))?;
                if let Some(state) = self.webgl.as_mut() {
                    state.shader.outputs.push(crate::webgl::glsl::WebGlInterface { name: name.into(), handle, ty, location: 0, explicit: false });
                }
                Ok(Some(var))
            }
            "gl_FragData" if fragment => {
                let base = ctx.module.types.insert(Type { name: None, inner: vec4 }, meta);
                let size = self.webgl.as_ref().map_or(1, |s| s.options.draw_buffers.max(1));
                let ty = ctx.module.types.insert(
                    Type {
                        name: None,
                        inner: TypeInner::Array {
                            base,
                            size: naga::ArraySize::Constant(core::num::NonZeroU32::new(size).unwrap_or(core::num::NonZeroU32::MIN)),
                            stride: 16,
                        },
                    },
                    meta,
                );
                let (handle, var) = self.webgl_global(ctx, name, ty, meta, Some(output(0)))?;
                if let Some(state) = self.webgl.as_mut() {
                    state.shader.frag_data = Some(handle);
                    state.shader.outputs.push(crate::webgl::glsl::WebGlInterface { name: name.into(), handle, ty, location: 0, explicit: false });
                }
                Ok(Some(var))
            }
            "gl_FragDepthEXT" if fragment => {
                self.add_builtin(ctx, name, data(TypeInner::Scalar(Scalar::F32), BuiltIn::FragDepth, true, StorageQualifier::Output), meta)
            }
            "gl_VertexID" | "gl_InstanceID" if !fragment => self.add_builtin(
                ctx,
                name,
                data(
                    TypeInner::Scalar(Scalar::I32),
                    match name {
                        "gl_VertexID" => BuiltIn::VertexIndex,
                        _ => BuiltIn::InstanceIndex,
                    },
                    false,
                    StorageQualifier::Input,
                ),
                meta,
            ),
            "gl_PointSize" if !fragment && self.webgl.as_ref().is_some_and(|s| s.shader.point_size.is_some()) => {
                let Some(handle) = self.webgl.as_ref().and_then(|s| s.shader.point_size) else { unreachable!() };
                let expr = ctx.add_expression(Expression::GlobalVariable(handle), meta)?;
                Ok(Some(VariableReference { expr, load: true, mutable: true, constant: None, entry_arg: None }))
            }
            "gl_PointSize" if !fragment => {
                let ty = ctx.module.types.insert(Type { name: None, inner: TypeInner::Scalar(Scalar::F32) }, meta);
                let (handle, var) = self.webgl_global(ctx, name, ty, meta, None)?;
                if let Some(state) = self.webgl.as_mut() {
                    state.shader.point_size = Some(handle);
                }
                Ok(Some(var))
            }
            _ => Ok(None),
        }
    }
}
