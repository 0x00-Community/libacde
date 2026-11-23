#![allow(non_upper_case_globals, non_camel_case_types, unused_imports, clippy::all)]

pub mod desktop {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("../protocol/acompositor/acompositor-desktop-v1.xml");
    }

    use self::interfaces::*;

    const _: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../protocol/acompositor/acompositor-desktop-v1.xml"));
    wayland_scanner::generate_client_code!("../protocol/acompositor/acompositor-desktop-v1.xml");
}

pub mod inject {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("../protocol/acompositor/acompositor-inject-v1.xml");
    }

    use self::interfaces::*;

    const _: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../protocol/acompositor/acompositor-inject-v1.xml"));
    wayland_scanner::generate_client_code!("../protocol/acompositor/acompositor-inject-v1.xml");
}
