use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub trait Loader {
    fn resolve(&self, specifier: &str, referrer: &str) -> Result<String, String>;
    fn load(&self, url: &str) -> Result<String, String>;
}

#[derive(Default)]
struct Map {
    loader: Option<Rc<dyn Loader>>,
    by_url: HashMap<String, v8::Global<v8::Module>>,
    by_hash: HashMap<i32, Vec<(v8::Global<v8::Module>, String)>>,
    builtins: HashMap<String, v8::Global<v8::Object>>,
    refused: HashMap<String, String>,
    inline_source: Option<v8::Global<v8::Function>>,
}

thread_local! {
    static MAP: RefCell<Map> = RefCell::new(Map::default());
    static IMPORT_MAP: RefCell<ImportMap> = RefCell::new(ImportMap::default());
}

type Specifiers = Vec<(String, String)>;

#[derive(Default)]
struct ImportMap {
    imports: Specifiers,
    scopes: Vec<(String, Specifiers)>,
}

impl ImportMap {
    fn lookup(&self, key: &str, referrer: &str) -> Option<String> {
        let mut scopes: Vec<&(String, Specifiers)> = self.scopes.iter().filter(|(prefix, _)| referrer.starts_with(prefix.as_str())).collect();
        scopes.sort_by_key(|(prefix, _)| std::cmp::Reverse(prefix.len()));
        scopes.into_iter().map(|(_, map)| map).chain(std::iter::once(&self.imports)).find_map(|map| Self::find(map, key))
    }

    fn find(map: &Specifiers, key: &str) -> Option<String> {
        if let Some((_, to)) = map.iter().find(|(from, _)| from == key) {
            return Some(to.clone());
        }
        map.iter()
            .filter(|(from, to)| from.ends_with('/') && to.ends_with('/') && key.starts_with(from.as_str()))
            .max_by_key(|(from, _)| from.len())
            .map(|(from, to)| format!("{to}{}", &key[from.len()..]))
    }
}

pub fn add_import_map(imports: Specifiers, scopes: Vec<(String, Specifiers)>) {
    IMPORT_MAP.with(|m| {
        let mut m = m.borrow_mut();
        for (from, to) in imports {
            if !m.imports.iter().any(|(f, _)| *f == from) {
                m.imports.push((from, to));
            }
        }
        for (prefix, map) in scopes {
            match m.scopes.iter_mut().find(|(p, _)| *p == prefix) {
                Some((_, existing)) => existing.extend(map.into_iter().filter(|(f, _)| !existing.iter().any(|(e, _)| e == f)).collect::<Vec<_>>()),
                None => m.scopes.push((prefix, map)),
            }
        }
    });
}

pub fn url_like(specifier: &str) -> bool {
    specifier.starts_with("./") || specifier.starts_with("../") || specifier.starts_with('/') || url::Url::parse(specifier).is_ok()
}

fn mapped(loader: &dyn Loader, specifier: &str, referrer: &str) -> Result<String, String> {
    let key = if url_like(specifier) { loader.resolve(specifier, referrer)? } else { specifier.to_owned() };
    match IMPORT_MAP.with(|m| m.borrow().lookup(&key, referrer)) {
        Some(found) => Ok(found),
        None if url_like(specifier) => Ok(key),
        None => loader.resolve(specifier, referrer),
    }
}

pub fn set_loader(loader: Rc<dyn Loader>) {
    MAP.with(|m| m.borrow_mut().loader = Some(loader));
}

fn loader() -> Result<Rc<dyn Loader>, String> {
    MAP.with(|m| m.borrow().loader.clone()).ok_or_else(|| "acdert: no module loader in this isolate".to_owned())
}

pub const SCHEME: &str = "acde:";

pub fn define(scope: &mut v8::PinScope, url: &str, exports: v8::Local<v8::Object>) {
    let exports = v8::Global::new(scope, exports);
    MAP.with(|m| {
        let mut m = m.borrow_mut();
        m.by_url.remove(url);
        m.builtins.insert(url.to_owned(), exports);
    });
}

pub fn refuse(url: &str, reason: &str) {
    MAP.with(|m| {
        let mut m = m.borrow_mut();
        m.by_url.remove(url);
        m.builtins.remove(url);
        m.refused.insert(url.to_owned(), reason.to_owned());
    });
}

fn builtin<'s>(scope: &mut v8::PinScope<'s, '_>, url: &str) -> Option<v8::Local<'s, v8::Object>> {
    MAP.with(|m| m.borrow().builtins.get(url).map(|g| v8::Local::new(scope, g)))
}

fn target(specifier: &str, referrer: &str) -> Result<String, String> {
    match specifier.starts_with(SCHEME) {
        true if MAP.with(|m| m.borrow().builtins.contains_key(specifier)) => Ok(specifier.to_owned()),
        true => Err(MAP
            .with(|m| m.borrow().refused.get(specifier).cloned())
            .unwrap_or_else(|| format!("acdert: '{specifier}' is not given to {referrer} (the capabilities in its manifest)"))),
        false => mapped(&*loader()?, specifier, referrer),
    }
}

fn names<'s>(scope: &mut v8::PinScope<'s, '_>, exports: v8::Local<'s, v8::Object>) -> Vec<v8::Local<'s, v8::String>> {
    let Some(keys) = exports.get_own_property_names(scope, Default::default()) else {
        return Vec::new();
    };
    (0..keys.length()).filter_map(|i| keys.get_index(scope, i)?.to_string(scope)).collect()
}

fn synthetic<'s>(scope: &mut v8::PinScope<'s, '_>, url: &str, exports: v8::Local<'s, v8::Object>) -> Option<v8::Local<'s, v8::Module>> {
    let names = names(scope, exports);
    let name = v8::String::new(scope, url)?;
    let module = v8::Module::create_synthetic_module(scope, name, &names, evaluate_builtin);
    remember(scope, url, module);
    Some(module)
}

fn evaluate_builtin<'s>(context: v8::Local<'s, v8::Context>, module: v8::Local<v8::Module>) -> Option<v8::Local<'s, v8::Value>> {
    v8::callback_scope!(unsafe scope, context);
    let url = url_of(scope, module)?;
    let exports = builtin(scope, &url)?;
    for name in names(scope, exports) {
        let value = exports.get(scope, name.into())?;
        module.set_synthetic_module_export(scope, name, value)?;
    }
    Some(v8::undefined(scope).into())
}

fn remember(scope: &mut v8::PinScope, url: &str, module: v8::Local<v8::Module>) {
    let global = v8::Global::new(scope, module);
    MAP.with(|m| {
        let mut m = m.borrow_mut();
        m.by_url.insert(url.to_owned(), global.clone());
        m.by_hash.entry(module.get_identity_hash().get()).or_default().push((global, url.to_owned()));
    });
}

fn url_of(scope: &mut v8::PinScope, module: v8::Local<v8::Module>) -> Option<String> {
    let hash = module.get_identity_hash().get();
    MAP.with(|m| {
        let m = m.borrow();
        m.by_hash.get(&hash)?.iter().find(|(g, _)| v8::Local::new(scope, g) == module).map(|(_, url)| url.clone())
    })
}

fn cached<'s>(scope: &mut v8::PinScope<'s, '_>, url: &str) -> Option<v8::Local<'s, v8::Module>> {
    MAP.with(|m| m.borrow().by_url.get(url).map(|g| v8::Local::new(scope, g)))
}

fn compile<'s>(scope: &mut v8::PinScope<'s, '_>, url: &str, source: &str) -> Option<v8::Local<'s, v8::Module>> {
    if let Some(module) = cached(scope, url) {
        return Some(module);
    }
    let name = v8::String::new(scope, url)?;
    let text = v8::String::new(scope, &crate::host::inspector::module_source(url, source))?;
    let origin = v8::ScriptOrigin::new(scope, name.into(), 0, 0, false, -1, None, false, false, true, None);
    let mut source = v8::script_compiler::Source::new(text, Some(&origin));
    let module = v8::script_compiler::compile_module(scope, &mut source)?;
    remember(scope, url, module);
    Some(module)
}

fn compile_graph<'s>(scope: &mut v8::PinScope<'s, '_>, url: &str) -> Option<v8::Local<'s, v8::Module>> {
    if let Some(module) = cached(scope, url) {
        return Some(module);
    }
    if let Some(exports) = builtin(scope, url) {
        return synthetic(scope, url, exports);
    }
    let source = match url.starts_with("blob:") || url.starts_with("data:") {
        true => inline_source(scope, url)?,
        false => thrown(scope, loader().and_then(|loader| loader.load(url)))?,
    };
    let module = compile(scope, url, &source)?;
    imports(scope, module, url)?;
    Some(module)
}

fn imports(scope: &mut v8::PinScope, module: v8::Local<v8::Module>, url: &str) -> Option<()> {
    let requests = module.get_module_requests();
    for i in 0..requests.length() {
        let request = requests.get(scope, i)?;
        let request = v8::Local::<v8::ModuleRequest>::try_from(request).ok()?;
        let specifier = request.get_specifier().to_rust_string_lossy(scope);
        let target = thrown(scope, target(&specifier, url))?;
        compile_graph(scope, &target)?;
    }
    Some(())
}

fn resolve_callback<'s>(
    context: v8::Local<'s, v8::Context>,
    specifier: v8::Local<'s, v8::String>,
    _attributes: v8::Local<'s, v8::FixedArray>,
    referrer: v8::Local<'s, v8::Module>,
) -> Option<v8::Local<'s, v8::Module>> {
    v8::callback_scope!(unsafe scope, context);
    let specifier = specifier.to_rust_string_lossy(scope);
    let referrer = url_of(scope, referrer).unwrap_or_default();
    let target = thrown(scope, target(&specifier, &referrer))?;
    let module = cached(scope, &target);
    thrown(scope, module.ok_or_else(|| format!("acdert: module {target} was not loaded")))
}

pub fn set_inline_source(function: Option<v8::Global<v8::Function>>) {
    MAP.with(|m| m.borrow_mut().inline_source = function);
}

fn inline_source(scope: &mut v8::PinScope, url: &str) -> Option<String> {
    let Some(function) = MAP.with(|m| m.borrow().inline_source.clone()) else {
        return thrown(scope, Err(format!("acdert: module {url}: this page reads no blob: or data: modules")));
    };
    let function = v8::Local::new(scope, function);
    let receiver = v8::undefined(scope).into();
    let arg = v8::String::new(scope, url)?.into();
    let source = function.call(scope, receiver, &[arg])?;
    if source.is_null_or_undefined() {
        return thrown(scope, Err(format!("acdert: module {url}: no such blob: or data: URL")));
    }
    Some(source.to_rust_string_lossy(scope))
}

fn thrown<T>(scope: &mut v8::PinScope, result: Result<T, String>) -> Option<T> {
    result.map_err(|e| crate::host::bind::throw_type_error(scope, &e)).ok()
}

fn link<'s>(scope: &mut v8::PinScope<'s, '_>, module: v8::Local<'s, v8::Module>) -> Option<v8::Local<'s, v8::Value>> {
    if module.get_status() == v8::ModuleStatus::Uninstantiated {
        module.instantiate_module(scope, resolve_callback)?;
    }
    module.evaluate(scope)
}

fn namespace_after<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    module: v8::Local<'s, v8::Module>,
    evaluation: v8::Local<'s, v8::Value>,
) -> Option<v8::Local<'s, v8::Promise>> {
    let namespace = module.get_module_namespace();
    let Ok(evaluation) = v8::Local::<v8::Promise>::try_from(evaluation) else {
        let resolver = v8::PromiseResolver::new(scope)?;
        resolver.resolve(scope, namespace);
        return Some(resolver.get_promise(scope));
    };
    let give = v8::Function::builder(|_: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue| {
        rv.set(args.data());
    })
    .data(namespace)
    .build(scope)?;
    evaluation.then(scope, give)
}

pub fn import<'s>(scope: &mut v8::PinScope<'s, '_>, url: &str) -> Option<v8::Local<'s, v8::Promise>> {
    evaluate(scope, |tc| compile_graph(tc, url))
}

pub fn run<'s>(scope: &mut v8::PinScope<'s, '_>, url: &str, source: &str) -> Option<v8::Local<'s, v8::Promise>> {
    evaluate(scope, |tc| {
        let module = compile(tc, url, source)?;
        imports(tc, module, url)?;
        Some(module)
    })
}

fn evaluate<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    compiled: impl for<'t> FnOnce(&mut v8::PinScope<'t, '_>) -> Option<v8::Local<'t, v8::Module>>,
) -> Option<v8::Local<'s, v8::Promise>> {
    let outcome = {
        v8::tc_scope!(let tc, scope);
        let done = compiled(tc).and_then(|module| {
            let evaluation = link(tc, module)?;
            Some((v8::Global::new(tc, module), v8::Global::new(tc, evaluation)))
        });
        match done {
            Some(done) => Ok(done),
            None => Err(tc.exception().map(|e| v8::Global::new(tc, e))),
        }
    };
    match outcome {
        Ok((module, evaluation)) => {
            let (module, evaluation) = (v8::Local::new(scope, module), v8::Local::new(scope, evaluation));
            namespace_after(scope, module, evaluation)
        }
        Err(exception) => {
            let exception = match exception {
                Some(e) => v8::Local::new(scope, e),
                None => v8::undefined(scope).into(),
            };
            let resolver = v8::PromiseResolver::new(scope)?;
            resolver.reject(scope, exception);
            Some(resolver.get_promise(scope))
        }
    }
}

pub fn dynamic_import<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    _options: v8::Local<'s, v8::Data>,
    resource: v8::Local<'s, v8::Value>,
    specifier: v8::Local<'s, v8::String>,
    _attributes: v8::Local<'s, v8::FixedArray>,
) -> Option<v8::Local<'s, v8::Promise>> {
    let specifier = specifier.to_rust_string_lossy(scope);
    let referrer = resource.to_rust_string_lossy(scope);
    match target(&specifier, &referrer) {
        Ok(url) => import(scope, &url),
        Err(e) => {
            let resolver = v8::PromiseResolver::new(scope)?;
            let message = crate::host::bind::string(scope, &e);
            let error = v8::Exception::type_error(scope, message);
            resolver.reject(scope, error);
            Some(resolver.get_promise(scope))
        }
    }
}

pub extern "C" fn import_meta(context: v8::Local<v8::Context>, module: v8::Local<v8::Module>, meta: v8::Local<v8::Object>) {
    v8::callback_scope!(unsafe scope, context);
    if let Some(url) = url_of(scope, module) {
        let key = v8::String::new(scope, "url").unwrap();
        let value = v8::String::new(scope, &url).unwrap();
        meta.create_data_property(scope, key.into(), value.into());
    }
}
