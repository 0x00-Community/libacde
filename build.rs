use std::path::{Path, PathBuf};

const HEADER: &str = "include/acdert.h";
const FFI: &str = "src/ffi.rs";
const PREFIX: &str = "acdert_";

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
            Self::Gnu => ("acdert.map", format!("{{\n  global:\n    {PREFIX}*;\n  local:\n    *;\n}};\n"), "-Wl,--exclude-libs,ALL,--version-script="),
            Self::Apple => ("acdert.exports", format!("_{PREFIX}*\n"), "-Wl,-exported_symbols_list,"),
            Self::Msvc => ("acdert.def", format!("EXPORTS\n{}\n", names.iter().map(|n| format!("  {n}")).collect::<Vec<_>>().join("\n")), "/DEF:"),
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
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-undefined,dynamic_lookup");
    }
    bundle_page(&dir, &out);
}

fn bundle_page(dir: &Path, out: &Path) {
    let level_file = dir.join("../npm/acde-api/api-level");
    for watched in ["js/page/src", "js/dom/src", "js/tsconfig.json", "../npm/src/manifest", "../npm/acde-api/api-level"] {
        println!("cargo:rerun-if-changed={watched}");
    }
    println!("cargo:rerun-if-env-changed=ACDEV_BUN");
    let level = std::fs::read_to_string(&level_file).expect("npm/acde-api/api-level");
    let version = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    let bun = std::env::var("ACDEV_BUN").unwrap_or_else(|_| "bun".into());
    let status = std::process::Command::new(&bun)
        .current_dir(dir.join("js"))
        .args(["build", "page/src/worker.ts", "--format", "esm", "--target", "browser", "--minify-whitespace", "--minify-syntax", "--keep-names"])
        .args(["--conditions", "source", "--define", &format!("ACDEV_API_LEVEL={}", level.trim())])
        .args(["--define", &format!("ACDEV_RUNTIME_VERSION=\"{version}\""), "--outfile"])
        .arg(out.join("page.js"))
        .status()
        .unwrap_or_else(|e| panic!("{bun}: {e}; bundling js/page needs bun (set ACDEV_BUN to its path)"));
    assert!(status.success(), "{bun} build js/page/src/worker.ts failed ({status})");
}
