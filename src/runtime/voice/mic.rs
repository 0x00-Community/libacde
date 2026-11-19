use pipewire as pw;
use pw::spa;

pub(super) const RATE: u32 = 16000;

pub(super) struct Mic {
    quit: pw::channel::Sender<()>,
}

impl Drop for Mic {
    fn drop(&mut self) {
        _ = self.quit.send(());
    }
}

pub(super) async fn open(id: u32) -> Result<(Mic, async_channel::Receiver<Vec<u8>>), String> {
    let (ready, opened) = async_channel::bounded(1);
    let (pcm, chunks) = async_channel::bounded(2000);
    std::thread::Builder::new()
        .name("acde-mic".into())
        .spawn(move || capture(id, pcm, ready))
        .map_err(|error| format!("cannot start the microphone thread: {error}"))?;
    let quit = opened.recv().await.map_err(|_| "the microphone thread stopped".to_owned())??;
    Ok((Mic { quit }, chunks))
}

fn capture(id: u32, pcm: async_channel::Sender<Vec<u8>>, ready: async_channel::Sender<Result<pw::channel::Sender<()>, String>>) {
    let started = (|| -> Result<_, pw::Error> {
        pw::init();
        let main = pw::main_loop::MainLoopRc::new(None)?;
        let context = crate::runtime::pw_context(&main)?;
        let core = context.connect_rc(None)?;
        let props = pw::properties::properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Communication",
            *pw::keys::APP_NAME => "acde",
            *pw::keys::NODE_NAME => "acde.voice",
            *pw::keys::NODE_LATENCY => format!("320/{RATE}"),
        };
        let stream = pw::stream::StreamRc::new(core.clone(), "acde-voice", props)?;
        let failed = pcm.clone();
        let listener = stream
            .add_local_listener_with_user_data(pcm)
            .state_changed(move |_, _, old, state| {
                match &state {
                    pw::stream::StreamState::Error(error) => log::warn!("voice #{id}: microphone error: {error}"),
                    pw::stream::StreamState::Unconnected | pw::stream::StreamState::Paused if old == pw::stream::StreamState::Streaming => {
                        log::warn!("voice #{id}: microphone dropped ({old:?} -> {state:?}), sound card lost or unplugged")
                    }
                    _ => log::info!("voice #{id}: microphone {old:?} -> {state:?}"),
                }
                if let pw::stream::StreamState::Error(_) = state {
                    failed.close();
                }
            })
            .process(|stream, pcm| {
                let Some(mut buffer) = stream.dequeue_buffer() else { return };
                let Some(data) = buffer.datas_mut().first_mut() else { return };
                let (offset, size) = (data.chunk().offset() as usize, data.chunk().size() as usize);
                if let Some(bytes) = data.data()
                    && let Some(samples) = bytes.get(offset..offset + size)
                    && !samples.is_empty()
                {
                    _ = pcm.try_send(samples.to_vec());
                }
            })
            .register()?;
        let mut info = spa::param::audio::AudioInfoRaw::new();
        info.set_format(spa::param::audio::AudioFormat::S16LE);
        info.set_rate(RATE);
        info.set_channels(1);
        let mut position = [0; spa::param::audio::MAX_CHANNELS];
        position[0] = spa::sys::SPA_AUDIO_CHANNEL_MONO;
        info.set_position(position);
        let object =
            spa::pod::Value::Object(spa::pod::Object { type_: spa::sys::SPA_TYPE_OBJECT_Format, id: spa::sys::SPA_PARAM_EnumFormat, properties: info.into() });
        let bytes =
            spa::pod::serialize::PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &object).map_err(|_| pw::Error::CreationFailed)?.0.into_inner();
        let pod = spa::pod::Pod::from_bytes(&bytes).ok_or(pw::Error::CreationFailed)?;
        stream.connect(spa::utils::Direction::Input, None, pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS, &mut [pod])?;
        let (quit, stop) = pw::channel::channel();
        Ok((main, context, core, stream, listener, quit, stop))
    })();
    let (main, _context, _core, _stream, _listener, quit, stop) = match started {
        Ok(started) => started,
        Err(error) => {
            log::warn!("voice #{id}: cannot open the microphone: PipeWire {error}");
            _ = ready.try_send(Err(format!("no microphone: PipeWire {error}")));
            return;
        }
    };
    let _stop = stop.attach(main.loop_(), {
        let main = main.clone();
        move |()| main.quit()
    });
    if ready.try_send(Ok(quit)).is_ok() {
        main.run();
    }
}
