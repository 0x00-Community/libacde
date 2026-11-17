use std::sync::OnceLock;

use base64::Engine;

fn runtime_bundles() -> &'static [String] {
    static URLS: OnceLock<Vec<String>> = OnceLock::new();
    URLS.get_or_init(|| {
        let dir = crate::host::web::app_dir();
        let dirs = [Some(dir.clone()), std::fs::canonicalize(&dir).ok()];
        let files = dirs.into_iter().flatten().map(|dir| dir.join("broker.js")).filter_map(|path| url::Url::from_file_path(path).ok()).map(String::from);
        std::iter::once(crate::host::web::PAGE_URL.to_owned()).chain(files).collect()
    })
}

pub(super) fn map_for(url: &str, source: &str, lines: usize) -> Option<String> {
    if !runtime_bundles().iter().any(|bundle| bundle == url) {
        return None;
    }
    let last = source.trim_end().rsplit('\n').next().unwrap_or_default();
    if last.starts_with("//# sourceMappingURL=") {
        return None;
    }
    Some(inline(lines))
}

fn inline(lines: usize) -> String {
    let mappings = format!("AAAA{}", ";AACA".repeat(lines.saturating_sub(1)));
    let map = serde_json::json!({
        "version": 3,
        "sources": ["acde-runtime"],
        "names": [],
        "mappings": mappings,
        "ignoreList": [0],
        "x_google_ignoreList": [0],
    });
    let encoded = base64::engine::general_purpose::STANDARD.encode(map.to_string());
    format!("\n//# sourceMappingURL=data:application/json;base64,{encoded}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_inline_map_ignore_lists_one_source_covering_every_line() {
        let comment = inline(3);
        let encoded = comment.strip_prefix("\n//# sourceMappingURL=data:application/json;base64,").expect("a data URL");
        let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).expect("base64");
        let map: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(map["mappings"], "AAAA;AACA;AACA");
        assert_eq!(
            (&map["sources"], &map["x_google_ignoreList"], &map["ignoreList"]),
            (&serde_json::json!(["acde-runtime"]), &serde_json::json!([0]), &serde_json::json!([0]))
        );
    }

    #[test]
    fn only_runtime_bundles_without_their_own_map_get_one() {
        let bundle = crate::host::web::PAGE_URL.to_owned();
        assert!(map_for(&bundle, "a()\nb()", 3).is_some());
        assert!(map_for(&bundle, "a()\n//# sourceMappingURL=page.js.map\n", 3).is_none(), "a dev build's own map wins");
        assert!(map_for("file:///elsewhere/app.js", "a()", 1).is_none(), "page scripts are left alone");
    }
}
