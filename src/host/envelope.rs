use crate::host::bind;
use crate::host::host::{deserialize_with, serialize_with};

pub struct Moved(v8::SharedRef<v8::BackingStore>);

unsafe impl Send for Moved {}

pub struct Envelope {
    bytes: Vec<u8>,
    buffers: Vec<Moved>,
}

fn refuse<T>(scope: &mut v8::PinScope, message: &str) -> Option<T> {
    bind::throw_error(scope, &format!("DataCloneError: {message}"));
    None
}

impl Envelope {
    pub fn pack(scope: &mut v8::PinScope, value: v8::Local<v8::Value>, transfer: &[v8::Local<v8::Value>]) -> Option<Envelope> {
        let mut buffers: Vec<v8::Local<v8::ArrayBuffer>> = Vec::with_capacity(transfer.len());
        for (i, item) in transfer.iter().enumerate() {
            let Ok(buffer) = v8::Local::<v8::ArrayBuffer>::try_from(*item) else {
                return refuse(scope, &format!("Value at index {i} does not have a transferable type."));
            };
            if buffers.iter().any(|b| b.strict_equals(buffer.into())) {
                return refuse(scope, &format!("ArrayBuffer at index {i} is a duplicate of an earlier ArrayBuffer."));
            }
            if buffer.was_detached() {
                return refuse(scope, &format!("ArrayBuffer at index {i} is already detached."));
            }
            if !buffer.is_detachable() {
                return refuse(scope, &format!("ArrayBuffer at index {i} is not detachable and could not be transferred."));
            }
            buffers.push(buffer);
        }
        let bytes = serialize_with(scope, value, &buffers)?;
        let mut moved = Vec::with_capacity(buffers.len());
        for buffer in buffers {
            moved.push(Moved(buffer.get_backing_store()));
            buffer.detach(None)?;
        }
        Some(Envelope { bytes, buffers: moved })
    }

    pub fn size(&self) -> usize {
        self.bytes.len() + self.buffers.iter().map(|b| b.0.byte_length()).sum::<usize>()
    }

    pub fn open<'s>(self, scope: &mut v8::PinScope<'s, '_>) -> Option<v8::Local<'s, v8::Value>> {
        let stores: Vec<v8::SharedRef<v8::BackingStore>> = self.buffers.into_iter().map(|m| m.0).collect();
        deserialize_with(scope, &self.bytes, &stores)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::mpsc::channel;

    use super::*;
    use crate::host::platform::Headless;
    use crate::host::rt::Runtime;

    fn eval<'s>(scope: &mut v8::PinScope<'s, '_>, code: &str) -> v8::Local<'s, v8::Value> {
        let code = v8::String::new(scope, code).expect("source");
        v8::Script::compile(scope, code, None).and_then(|s| s.run(scope)).expect("ran")
    }

    fn list<'s>(scope: &mut v8::PinScope<'s, '_>, code: &str) -> Vec<v8::Local<'s, v8::Value>> {
        let array = v8::Local::<v8::Array>::try_from(eval(scope, code)).expect("array");
        (0..array.length()).map(|i| array.get_index(scope, i).expect("item")).collect()
    }

    fn text(scope: &mut v8::PinScope, code: &str) -> String {
        let value = eval(scope, code);
        value.to_rust_string_lossy(scope)
    }

    #[test]
    fn a_transferred_buffer_moves_without_a_copy_and_detaches_the_sender() {
        let mut rt = Runtime::new(None, "envelope", Arc::new(Headless));
        rt.task(|scope| {
            let value = eval(scope, "globalThis.b = new ArrayBuffer(16); new Float32Array(b)[1] = 2.5; ({ a: new Float32Array(b), c: new Uint8Array(b, 4) })");
            let before = eval(scope, "b");
            let pointer = v8::Local::<v8::ArrayBuffer>::try_from(before).expect("buffer").data();
            let transfer = list(scope, "[b]");
            let envelope = Envelope::pack(scope, value, &transfer).expect("packed");
            assert_eq!(envelope.size() >= 16, true);
            assert_eq!(text(scope, "String([b.detached, b.byteLength])"), "true,0");
            let opened = envelope.open(scope).expect("opened");
            let global = scope.get_current_context().global(scope);
            let key = v8::String::new(scope, "got").expect("key");
            global.set(scope, key.into(), opened);
            assert_eq!(text(scope, "String([got.a[1], got.a.buffer === got.c.buffer, got.c.byteOffset])"), "2.5,true,4");
            let after = eval(scope, "got.a.buffer");
            assert_eq!(v8::Local::<v8::ArrayBuffer>::try_from(after).expect("buffer").data(), pointer, "the same memory");
        });
    }

    #[test]
    fn a_refused_transfer_detaches_nothing() {
        let mut rt = Runtime::new(None, "refusals", Arc::new(Headless));
        rt.task(|scope| {
            eval(scope, "globalThis.b = new ArrayBuffer(8); globalThis.m = new WebAssembly.Memory({ initial: 1 }).buffer; globalThis.d = new ArrayBuffer(4); d.transfer()");
            for (list_code, needle) in [("[b, b]", "duplicate"), ("[b, m]", "not detachable"), ("[b, d]", "already detached"), ("[b, 5]", "transferable type")] {
                let transfer = list(scope, list_code);
                let value = eval(scope, "({ b })");
                v8::tc_scope!(let tc, scope);
                assert!(Envelope::pack(tc, value, &transfer).is_none());
                let message = tc.exception().map(|e| e.to_rust_string_lossy(tc)).unwrap_or_default();
                assert!(message.contains("DataCloneError") && message.contains(needle), "{message}");
            }
            assert_eq!(text(scope, "String([b.detached, b.byteLength])"), "false,8");
        });
    }

    #[test]
    fn a_buffer_outlives_the_isolate_that_sent_it() {
        let (tx, rx) = channel::<Envelope>();
        std::thread::spawn(move || {
            let mut rt = Runtime::new(None, "sender", Arc::new(Headless));
            let envelope = rt.task(|scope| {
                let value = eval(scope, "globalThis.b = new ArrayBuffer(1 << 20); new Uint32Array(b).fill(0xabcdef); new Uint32Array(b)");
                let transfer = list(scope, "[b]");
                Envelope::pack(scope, value, &transfer).expect("packed")
            });
            drop(rt);
            tx.send(envelope).expect("sent");
        })
        .join()
        .expect("sender thread");
        let envelope = rx.recv().expect("received");
        std::thread::spawn(move || {
            let mut rt = Runtime::new(None, "receiver", Arc::new(Headless));
            rt.task(|scope| {
                let opened = envelope.open(scope).expect("opened");
                let global = scope.get_current_context().global(scope);
                let key = v8::String::new(scope, "got").expect("key");
                global.set(scope, key.into(), opened);
                assert_eq!(text(scope, "String([got.length, got.every((v) => v === 0xabcdef)])"), "262144,true");
            });
        })
        .join()
        .expect("receiver thread");
    }
}
