# Sound, video, camera and device state

Synced with docs/agents on 2026-11-24

Pages play sound and video, record from cameras and microphones, and read the state of the machine (battery, idle, wake locks) through the standard web interfaces. libacde decodes and mixes in Rust, uses the operating system's own codecs for video, and never adds private methods to these interfaces. Settings that the web has no interface for (volume of a device, power profiles, Bluetooth) are `acde:` modules, described in [system modules](../apps/system-modules.md).

## Sound: `Audio` and Web Audio

- `new Audio()`, `<audio>` and `AudioContext` decode with symphonia and mix in a 48 kHz stereo float mixer, one per page, on its own thread. Decoding and mixing are identical on every platform.
- Output goes to PipeWire on Linux and to the default device (through cpal) on macOS and Windows, resampled to the device's rate. The stream closes after 3 seconds of silence. With no device, or with `ACDE_AUDIO=null`, audio is rendered against the wall clock and dropped, so timing still behaves.
- **`OfflineAudioContext`** renders as fast as it can at any sample rate from 3000 to 768000 Hz; `decodeAudioData` resamples to the context's rate.
- **Nodes**: oscillators (the four basic waves and custom `PeriodicWave`), `PannerNode` and `AudioListener` (azimuth, elevation, three distance models, cones, `equalpower`), `BiquadFilterNode`, `DelayNode` (up to 180 s), `WaveShaperNode` (with 2x / 4x oversampling), `DynamicsCompressorNode`, `ConvolverNode` (no added latency) and `AnalyserNode`. The filter, shaper and compressor follow Chromium's algorithms, and the same scripts were compared against Chrome when they were written. Nodes keep sounding for their natural tail after the input goes silent (the delay time, 1.625 s for the compressor, the impulse length for a convolver).
- The mixer's health can be watched with `RUST_LOG=perf::audio=debug`: one line a second with level, peak and frames that had to be filled with silence.

## Video: `<video>`

- Video is decoded by the operating system's media framework, never by a bundled codec: GStreamer on Linux, AVFoundation on macOS. Containers recognised on Linux are MP4 / QuickTime, Matroska / WebM and Ogg; on macOS a file that refers to other files or URLs is not followed.
- Same-origin files (from the package, or a folder lent by the system, such as a wallpaper folder) are opened as files and read by byte range, never loaded whole into memory. Elements created by parsing (`innerHTML`, the page HTML) and by cloning play like ones made with `createElement`.
- Each frame is uploaded to the GPU once; zero-copy hardware decoding (dmabuf, IOSurface) is not done yet.
- There is no video backend on Windows: no video type is playable there and opening one fails with "video playback is not supported on this platform".

## Camera and microphone: `getUserMedia`

- **Permission**: the app must declare `camera` and / or `microphone` in its capabilities, and the user must allow it (see [approval](../apps/manifest.md#approval-what-asks-the-user)). Device selection happens first, without asking: an impossible constraint fails with `NotFoundError` or `OverconstrainedError`, a known refusal with `NotAllowedError`. Then the user is asked once per kind.
- `navigator.permissions.query({name: 'camera'})` reports the system's state once approved, and `prompt` while not yet asked. Until a kind is approved, `enumerateDevices` shows a single entry with an empty id and label for it.
- **Devices**: GStreamer's device monitor on Linux (PipeWire and V4L2). Constraints are applied as the standard's SelectSettings: required `exact` / `min` / `max`, then `advanced`, then distance to `ideal`.
- **Playing a stream**: `video.srcObject = stream` shows the camera's newest frame each animation frame; an `<audio>` element only plays the sound.
- **Testing without hardware**: `ACDE_CAPTURE=fake` replaces the devices with a fake microphone (a 440 Hz tone) and a fake camera with a few resolutions, like Chrome's fake devices.
- **Not yet**: `devicechange`, screen capture (`getDisplayMedia`), `MediaRecorder`, `createMediaStreamDestination`, echo cancellation and other processing, and ending tracks that were already running when permission is revoked (new tracks are refused). Camera and microphone are not available on Windows hosts.

## Device state through web standards

- **Battery** (`navigator.getBattery()`): on Linux it reads UPower's display device and fires `chargingchange`, `levelchange`, `chargingtimechange` and `dischargingtimechange` as they happen. Without a battery, without UPower, or on other platforms it reports what the standard gives for mains power (charging, level 1).
- **Screen wake lock** (`navigator.wakeLock.request('screen')`, Linux only): holds logind's idle inhibitor (which acdes's power policy honours) and the session's `org.freedesktop.ScreenSaver` inhibition (which GNOME and KDE honour). A hidden page cannot take one, and every lock is released with a `release` event when the page becomes hidden. Elsewhere `navigator.wakeLock` does not exist.
- **Idle detection** (`IdleDetector`, Linux only): reports the user as idle once the session has been idle for the threshold (at least one minute) and the screen as locked when the session is locked, following logind. The desktop decides when the session counts as idle, so the detector cannot see idleness sooner than the desktop reports it. It needs the `idleDetection` capability, and `IdleDetector.requestPermission()` asks the user.

## Voice input

On Linux the runtime can record from the default PipeWire input and stream it to an online speech recogniser, delivering partial text and exactly one final result or error per recording; a recording stops after 60 seconds, and a new one cancels the previous. This is not a web interface: it is the shell module `acde:voice`, given only to desktop services that declare it (see [the desktop package](../desktop/desktop-package.md)). The recognised text is never written to the log.

## For developers

[web/web-apps.md](../../../agents/web/web-apps.md) ("声音", "视频", "摄像头和麦克风", "设备的标准接口", "语音输入"), [dev/build-run.md](../../../agents/dev/build-run.md) (logs and `perf::audio`).
