use std::path::Path;
use std::sync::OnceLock;

pub const GUEST_KEY: &str = "/sys/firmware/qemu_fw_cfg/by_name/opt/acdev/key/raw";

pub fn managed() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("ACDE_MANAGER_FD").is_some())
}

pub fn dev_mode() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| decide(managed(), || Path::new(GUEST_KEY).exists(), std::env::var_os("ACDE_DEV").is_some()))
}

fn decide(managed: bool, guest: impl FnOnce() -> bool, asked: bool) -> bool {
    if managed { guest() } else { asked }
}

#[cfg(test)]
mod tests {
    use super::decide;

    #[test]
    fn a_managed_process_is_in_dev_mode_only_in_an_acdev_guest_whatever_its_environment_says() {
        assert!(!decide(true, || false, true));
        assert!(decide(true, || true, false));
        assert!(decide(true, || true, true));
    }

    #[test]
    fn a_process_with_no_manager_follows_acde_dev() {
        assert!(decide(false, || panic!("the guest key is not looked at"), true));
        assert!(!decide(false, || panic!("the guest key is not looked at"), false));
    }
}
