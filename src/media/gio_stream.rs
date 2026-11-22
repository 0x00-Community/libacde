use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use gio::glib;
use gio::subclass::prelude::*;

use crate::media::Source;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct SourceStream {
        pub(super) source: OnceLock<Source>,
        pub(super) position: AtomicU64,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SourceStream {
        const NAME: &'static str = "AcdeMediaSourceStream";
        type Type = super::SourceStream;
        type ParentType = gio::InputStream;
        type Interfaces = (gio::Seekable,);
    }

    impl ObjectImpl for SourceStream {}

    impl InputStreamImpl for SourceStream {
        fn read(&self, buffer: &mut [u8], _: Option<&gio::Cancellable>) -> Result<usize, glib::Error> {
            let source = self.source.get().ok_or_else(|| failure(gio::IOErrorEnum::Closed, "the media source is closed"))?;
            let position = self.position.load(Ordering::Acquire);
            let n = source.read_at(position, buffer).map_err(|e| failure(gio::IOErrorEnum::Failed, &e.to_string()))?;
            self.position.store(position + n as u64, Ordering::Release);
            Ok(n)
        }
    }

    impl SeekableImpl for SourceStream {
        fn tell(&self) -> i64 {
            i64::try_from(self.position.load(Ordering::Acquire)).unwrap_or(i64::MAX)
        }

        fn can_seek(&self) -> bool {
            true
        }

        fn seek(&self, offset: i64, kind: glib::SeekType, _: Option<&gio::Cancellable>) -> Result<(), glib::Error> {
            let base = match kind {
                glib::SeekType::Set => 0,
                glib::SeekType::Cur => self.position.load(Ordering::Acquire),
                glib::SeekType::End => self.source.get().map_or(0, Source::len),
                _ => return Err(failure(gio::IOErrorEnum::InvalidArgument, "unknown seek type")),
            };
            let target = base.checked_add_signed(offset).ok_or_else(|| failure(gio::IOErrorEnum::InvalidArgument, "seeking outside the media"))?;
            self.position.store(target, Ordering::Release);
            Ok(())
        }

        fn can_truncate(&self) -> bool {
            false
        }

        fn truncate(&self, _: i64, _: Option<&gio::Cancellable>) -> Result<(), glib::Error> {
            Err(failure(gio::IOErrorEnum::NotSupported, "a media source is read only"))
        }
    }

    fn failure(kind: gio::IOErrorEnum, message: &str) -> glib::Error {
        glib::Error::new(kind, message)
    }
}

glib::wrapper! {
    pub struct SourceStream(ObjectSubclass<imp::SourceStream>) @extends gio::InputStream, @implements gio::Seekable;
}

impl SourceStream {
    pub fn new(source: Source) -> Self {
        let stream: Self = glib::Object::new();
        _ = stream.imp().source.set(source);
        stream
    }
}
