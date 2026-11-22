use std::collections::HashMap;
use std::sync::OnceLock;

const LEGACY: &str = include_str!("../acde-api/legacy-icons.json");

fn legacy() -> &'static HashMap<String, String> {
    static TABLE: OnceLock<HashMap<String, String>> = OnceLock::new();
    TABLE.get_or_init(|| serde_json::from_str(LEGACY).unwrap_or_default())
}

pub fn standard(icon: &str) -> &str {
    legacy().get(icon).map_or(icon, String::as_str)
}

pub fn is_legacy(icon: &str) -> bool {
    legacy().contains_key(icon)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_names_become_freedesktop_names_and_others_stay() {
        assert_eq!(standard("files"), "system-file-manager");
        assert_eq!(standard("dopa"), "dopame-boss");
        assert_eq!(standard("accessories-calculator"), "accessories-calculator");
        assert!(is_legacy("calc"));
        assert!(!is_legacy("/app/x/icon.svg"));
        assert_eq!(legacy().len(), 29);
    }
}
