use std::sync::OnceLock;

use crate::host::{bind, offload};

const HASHERS: usize = 2;

fn hashers() -> &'static offload::Pool {
    static POOL: OnceLock<offload::Pool> = OnceLock::new();
    POOL.get_or_init(|| offload::Pool::new("acde-digest", HASHERS))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Algorithm {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

impl Algorithm {
    fn named(name: &str) -> Option<Algorithm> {
        match name {
            "SHA-1" => Some(Algorithm::Sha1),
            "SHA-256" => Some(Algorithm::Sha256),
            "SHA-384" => Some(Algorithm::Sha384),
            "SHA-512" => Some(Algorithm::Sha512),
            _ => None,
        }
    }

    fn digest(self, data: &[u8]) -> Vec<u8> {
        use sha1::Digest as _;
        use sha2::Digest as _;
        match self {
            Algorithm::Sha1 => sha1::Sha1::digest(data).to_vec(),
            Algorithm::Sha256 => sha2::Sha256::digest(data).to_vec(),
            Algorithm::Sha384 => sha2::Sha384::digest(data).to_vec(),
            Algorithm::Sha512 => sha2::Sha512::digest(data).to_vec(),
        }
    }
}

pub fn digest(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let name = bind::arg_string(scope, &args, 0);
    let Some(algorithm) = Algorithm::named(&name) else {
        return bind::throw_type_error(scope, &format!("digest: unknown algorithm {name}"));
    };
    let Some(data) = bind::bytes(args.get(1)) else {
        return bind::throw_type_error(scope, "digest: data is not an ArrayBuffer or a view");
    };
    hashers().run(scope, &mut rv, move || algorithm.digest(&data), settle);
}

fn settle(scope: &mut v8::PinScope, resolver: v8::Local<v8::PromiseResolver>, digest: Vec<u8>) {
    let store = v8::ArrayBuffer::new_backing_store_from_vec(digest).make_shared();
    let buffer = v8::ArrayBuffer::with_backing_store(scope, &store);
    resolver.resolve(scope, buffer.into());
}

#[cfg(test)]
mod tests {
    use super::Algorithm;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn digests_match_the_fips_vectors() {
        let abc = b"abc";
        assert_eq!(hex(&Algorithm::Sha1.digest(abc)), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(hex(&Algorithm::Sha256.digest(abc)), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(hex(&Algorithm::Sha384.digest(abc)), "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7");
        assert_eq!(
            hex(&Algorithm::Sha512.digest(abc)),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
        assert_eq!(hex(&Algorithm::Sha256.digest(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    #[test]
    fn only_normalized_names_are_known() {
        assert_eq!(Algorithm::named("SHA-256"), Some(Algorithm::Sha256));
        assert_eq!(Algorithm::named("sha-256"), None);
        assert_eq!(Algorithm::named("MD5"), None);
    }
}
