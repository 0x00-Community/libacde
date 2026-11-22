use std::path::{Path, PathBuf};

const HEADER: &str = "include/acde.h";
const FFI: &str = "src/ffi.rs";
const PREFIX: &str = "acde_";

enum Linker {
    Gnu,
    Apple,
    Msvc,
}

impl Linker {
    fn current() -> Self {
        match (std::env::var("CARGO_CFG_TARGET_OS").as_deref(), std::env::var("CARGO_CFG_TARGET_ENV").as_deref()) {
            (Ok("macos" | "ios"), _) => Self::Apple,
            (_, Ok("msvc")) => Self::Msvc,
            _ => Self::Gnu,
        }
    }

    fn exports(&self, names: &[String], out: &Path) {
        let (file, text, arg) = match self {
            Self::Gnu => ("acde.map", format!("{{\n  global:\n    {PREFIX}*;\n  local:\n    *;\n}};\n"), "-Wl,--exclude-libs,ALL,--version-script="),
            Self::Apple => ("acde.exports", format!("_{PREFIX}*\n"), "-Wl,-exported_symbols_list,"),
            Self::Msvc => ("acde.def", format!("EXPORTS\n{}\n", names.iter().map(|n| format!("  {n}")).collect::<Vec<_>>().join("\n")), "/DEF:"),
        };
        let path = out.join(file);
        std::fs::write(&path, text).expect("export list");
        println!("cargo:rustc-cdylib-link-arg={arg}{}", path.display());
    }
}

fn main() {
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    for watched in ["src/lib.rs", FFI, "cbindgen.toml"] {
        println!("cargo:rerun-if-changed={watched}");
    }
    let config = cbindgen::Config::from_file(dir.join("cbindgen.toml")).expect("cbindgen.toml");
    cbindgen::Builder::new().with_config(config).with_src(dir.join(FFI)).generate().expect("cbindgen: src/ffi.rs").write_to_file(dir.join(HEADER));
    let source = std::fs::read_to_string(dir.join(FFI)).expect("src/ffi.rs");
    let names: Vec<String> = source
        .lines()
        .filter_map(|line| line.split_once("extern \"C\" fn ").map(|(_, rest)| rest))
        .filter(|rest| rest.starts_with(PREFIX))
        .map(|rest| rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect())
        .collect();
    Linker::current().exports(&names, &out);
    if matches!(Linker::current(), Linker::Msvc) {
        napi_exports(&dir, &out);
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-undefined,dynamic_lookup");
    }
    bundle_js(&dir, &out);
}

fn bundle_js(dir: &Path, out: &Path) {
    let watched =
        ["js/bundle.ts", "js/page/src", "js/dom/src", "js/launcher/src", "js/tsconfig.json", "manifest", "acde-api/api-level", "acde-api/min-api-level"];
    for watched in watched {
        println!("cargo:rerun-if-changed={watched}");
    }
    println!("cargo:rerun-if-env-changed=ACDEV_BUN");
    let version = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    let bun = std::env::var("ACDEV_BUN").unwrap_or_else(|_| "bun".into());
    let status = std::process::Command::new(&bun)
        .current_dir(dir.join("js"))
        .arg("bundle.ts")
        .arg(out)
        .arg(&version)
        .status()
        .unwrap_or_else(|e| panic!("{bun}: {e}; bundling js/ needs bun (set ACDEV_BUN to its path)"));
    assert!(status.success(), "{bun} js/bundle.ts failed ({status})");
}

fn napi_exports(dir: &Path, out: &Path) {
    let sources = dir.join("src/napi");
    println!("cargo:rerun-if-changed={}", sources.display());
    let texts: Vec<String> = std::fs::read_dir(&sources)
        .expect("src/napi")
        .map(|entry| entry.expect("src/napi entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == "rs"))
        .map(|path| std::fs::read_to_string(&path).expect("src/napi source"))
        .collect();
    let mut names: Vec<String> = texts.iter().flat_map(|text| napi_names(text, &napi_makers(&texts))).collect();
    names.sort();
    names.dedup();
    let def = out.join("napi.def");
    std::fs::write(&def, format!("EXPORTS\n{}\n", names.iter().map(|n| format!("  {n}")).collect::<Vec<_>>().join("\n"))).expect("napi.def");
    println!("cargo::metadata=napi_def={}", def.display());
}

fn ident(text: &str) -> String {
    text.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect()
}

fn napi_makers(texts: &[String]) -> Vec<String> {
    let mut makers = Vec::new();
    for text in texts {
        let mut maker: Option<String> = None;
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("macro_rules! ") {
                maker = Some(ident(rest));
            } else if line.trim() == "#[napi_sym]" {
                makers.extend(maker.take());
            } else if line == "}" {
                maker = None;
            }
        }
    }
    makers
}

fn napi_names(text: &str, makers: &[String]) -> Vec<String> {
    let mut names = Vec::new();
    let (mut exported, mut block) = (false, false);
    for line in text.lines() {
        if block {
            if line == "}" {
                block = false;
            } else if let Some(entry) = line.strip_prefix("    ").filter(|e| !e.starts_with(' ')) {
                let name = ident(entry);
                if name.starts_with("napi_") || name.starts_with("node_api_") {
                    names.push(name);
                }
            }
            continue;
        }
        if line.strip_suffix("! {").is_some_and(|called| makers.iter().any(|m| m == called)) {
            block = true;
            continue;
        }
        let line = line.trim();
        if line == "#[napi_sym]" || line == "#[unsafe(no_mangle)]" {
            exported = true;
            continue;
        }
        if !exported || line.starts_with("#[") {
            continue;
        }
        exported = false;
        let Some((_, rest)) = line.split_once("fn ") else { continue };
        let name = ident(rest);
        if !name.is_empty() && !name.starts_with('$') {
            names.push(name);
        }
    }
    names
}
