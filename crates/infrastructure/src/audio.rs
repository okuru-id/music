use std::collections::VecDeque;
use std::fs::File;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use domain::{FilePath, Volume};
use symphonia::core::codecs::audio::{AudioCodecParameters, AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Time;

use application::{AudioEngine, AudioError};

/// Samples pushed to the device per chunk (~85ms @48kHz stereo).
const CHUNK_SAMPLES: usize = 8192;
/// Chunk-channel capacity ≈ 5s of buffered audio.
const CHANNEL_DEPTH: usize = 64;

// ponytail: decode-thread + bounded chunk channel instead of a lock-free SPSC
// ring; backpressure is per-chunk (not per-sample) so the device callback does
// no allocation and no blocking. Swap to `ringbuf` only if underruns are
// measured on weak hardware.

/// State shared between the API, the decode thread and the device callback.
struct Shared {
    /// Bumped on load/seek/gapless-advance; stale chunks are dropped by epoch.
    epoch: AtomicU64,
    /// Position at the start of the current epoch (ms).
    base_ms: AtomicU64,
    /// Frames consumed by the device this epoch (authoritative position).
    frames_this_epoch: AtomicU64,
    playing: AtomicBool,
    /// A track is loaded (drives "stopped" vs "paused" for auto-advance).
    active: AtomicBool,
    volume_bits: AtomicU32,
    /// Gapless preload target, consumed by the decode thread at EOF.
    next_path: std::sync::Mutex<Option<String>>,
    device_rate: u32,
    device_channels: usize,
}

impl Shared {
    fn position_ms(&self) -> u64 {
        let frames = self.frames_this_epoch.load(Ordering::Relaxed);
        self.base_ms.load(Ordering::Relaxed)
            + frames.saturating_mul(1000) / u64::from(self.device_rate.max(1))
    }
}

struct Session {
    reader: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    /// Output spec, learned from the first decoded buffer.
    file_rate: u32,
    file_channels: usize,
    /// Converted samples (device rate/channels) awaiting dispatch.
    carry: Vec<f32>,
}

impl Session {
    /// Decode one packet into `carry`. Returns false at EOF.
    fn decode_step(&mut self, shared: &Shared) -> Result<bool, SymphoniaError> {
        let Some(packet) = self.reader.next_packet()? else {
            return Ok(false);
        };
        let buf = self.decoder.decode(&packet)?;
        if self.file_rate == 0 {
            self.file_rate = buf.spec().rate();
            self.file_channels = buf.spec().channels().count();
        }
        let mut samples = vec![0.0f32; buf.samples_interleaved()];
        buf.copy_to_slice_interleaved(&mut samples);

        let mapped = map_channels(&samples, self.file_channels, shared.device_channels);
        let converted = if self.file_rate == shared.device_rate {
            mapped
        } else {
            resample_linear(
                &mapped,
                self.file_rate,
                shared.device_rate,
                shared.device_channels,
            )
        };
        self.carry.extend(converted);
        Ok(true)
    }

    fn seek_to_ms(&mut self, ms: u64) -> Result<(), SymphoniaError> {
        self.reader.seek(
            SeekMode::Accurate,
            SeekTo::Time {
                time: Time::from_millis_u64(ms),
                track_id: Some(self.track_id),
            },
        )?;
        self.decoder.reset();
        self.carry.clear();
        Ok(())
    }
}

/// Open + probe + make decoder; reads the output spec from the first packet
/// (its samples are discarded — ~20ms, inaudible, keeps open() simple).
fn open_file(path: &str) -> Result<Session, AudioError> {
    let load = |e: String| AudioError::Load(path.to_string(), e);
    let src = File::open(path).map_err(|e| load(e.to_string()))?;
    let mss = MediaSourceStream::new(
        Box::new(src),
        symphonia::core::io::MediaSourceStreamOptions::default(),
    );
    let mut hint = Hint::new();
    if let Some(ext) = FilePath(path.to_string()).extension() {
        hint.with_extension(&ext);
    }
    // Gapless trimming is on by default (AudioDecoderOptions::gapless = true):
    // MP3/OGG solid; M4A gap known, symphonia #544.
    let format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| load(e.to_string()))?;

    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| load("no audio track".into()))?;
    let track_id = track.id;
    let params: &AudioCodecParameters = track
        .codec_params
        .as_ref()
        .and_then(|c| c.audio())
        .ok_or_else(|| load("no audio codec params".into()))?;
    let decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|e| load(e.to_string()))?;

    let mut session = Session {
        reader: format,
        decoder,
        track_id,
        file_rate: 0,
        file_channels: 0,
        carry: Vec::new(),
    };
    let packet = session
        .reader
        .next_packet()
        .map_err(|e| load(e.to_string()))?
        .ok_or_else(|| load("empty stream".into()))?;
    let buf = session
        .decoder
        .decode(&packet)
        .map_err(|e| load(e.to_string()))?;
    session.file_rate = buf.spec().rate();
    session.file_channels = buf.spec().channels().count();
    Ok(session)
}

enum Cmd {
    Swap(Session, mpsc::SyncSender<()>),
    Seek { ms: u64, done: mpsc::SyncSender<()> },
}

enum Flow {
    /// Keep decoding the current session.
    Continue,
    /// New session installed: restart from the decode step.
    Reset,
}

fn handle_cmd(session: &mut Option<Session>, cmd: Cmd, shared: &Shared) -> Flow {
    match cmd {
        Cmd::Swap(s, done) => {
            *session = Some(s);
            shared.active.store(true, Ordering::Relaxed);
            shared.base_ms.store(0, Ordering::Relaxed);
            shared.epoch.fetch_add(1, Ordering::Relaxed);
            let _ = done.send(());
            Flow::Reset
        }
        Cmd::Seek { ms, done } => {
            if let Some(s) = session {
                let _ = s.seek_to_ms(ms);
            }
            shared.base_ms.store(ms, Ordering::Relaxed);
            shared.epoch.fetch_add(1, Ordering::Relaxed);
            let _ = done.send(());
            Flow::Continue
        }
    }
}

fn decoder_loop(
    cmd_rx: mpsc::Receiver<Cmd>,
    chunk_tx: mpsc::SyncSender<(u64, Vec<f32>)>,
    shared: Arc<Shared>,
) {
    let mut session: Option<Session> = None;
    let mut idle = false;
    'outer: loop {
        // 1. Pending commands are always handled first.
        loop {
            match cmd_rx.try_recv() {
                Err(mpsc::TryRecvError::Disconnected) => return,
                Err(mpsc::TryRecvError::Empty) => break,
                Ok(cmd) => match handle_cmd(&mut session, cmd, &shared) {
                    Flow::Reset => {
                        idle = false;
                        break;
                    }
                    Flow::Continue => {}
                },
            }
        }

        // 2. No session: wait for one.
        let Some(s) = session.as_mut() else {
            match cmd_rx.recv_timeout(Duration::from_millis(200)) {
                Ok(_) => continue,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        };

        // 3. Decode while idle-false; on EOF gapless-advance or go idle.
        if idle {
            thread::sleep(Duration::from_millis(50));
            continue 'outer;
        }
        match s.decode_step(&shared) {
            Ok(true) => {}
            Ok(false) => {
                let next = shared.next_path.lock().unwrap().take();
                match next.map(|p| open_file(&p)) {
                    Some(Ok(next_session)) => {
                        // Continuous sample stream; epoch bump resets position.
                        session = Some(next_session);
                        shared.base_ms.store(0, Ordering::Relaxed);
                        shared.epoch.fetch_add(1, Ordering::Relaxed);
                    }
                    Some(Err(e)) => {
                        eprintln!("audio: gapless load failed: {e}");
                        idle = true;
                        shared.active.store(false, Ordering::Relaxed);
                    }
                    None => {
                        idle = true;
                        shared.active.store(false, Ordering::Relaxed);
                    }
                }
                continue 'outer;
            }
            // ponytail: decode errors stop the track silently; surface a
            // PlaybackError event when the queue use case lands.
            Err(e) => {
                eprintln!("audio: decode error: {e}");
                idle = true;
                shared.active.store(false, Ordering::Relaxed);
                continue 'outer;
            }
        }

        // 4. Stage a bounded batch inside the `s` borrow, then dispatch
        //    outside it so commands stay responsive while the channel is full.
        const BATCH_CHUNKS: usize = 16; // ~1.4s staged ahead of the channel
        let mut batch: Vec<Vec<f32>> = Vec::new();
        while batch.len() < BATCH_CHUNKS && s.carry.len() >= CHUNK_SAMPLES {
            batch.push(s.carry.drain(..CHUNK_SAMPLES).collect());
        }
        for staged in batch {
            let mut chunk = staged;
            let epoch = shared.epoch.load(Ordering::Relaxed);
            loop {
                match chunk_tx.try_send((epoch, chunk)) {
                    Ok(()) => break,
                    Err(mpsc::TrySendError::Full(back)) => {
                        chunk = back.1;
                        match cmd_rx.try_recv() {
                            Err(mpsc::TryRecvError::Disconnected) => return,
                            Ok(cmd) => match handle_cmd(&mut session, cmd, &shared) {
                                Flow::Reset => {
                                    idle = false;
                                    continue 'outer;
                                }
                                Flow::Continue => {}
                            },
                            Err(mpsc::TryRecvError::Empty) => {}
                        }
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(mpsc::TrySendError::Disconnected(_)) => return,
                }
            }
        }
    }
}

/// Map `in_ch`-channel interleaved frames onto `out_ch` channels.
/// Extra device channels duplicate the last source channel; extra source
/// channels are truncated. ponytail: real downmix when 5.1 files matter.
fn map_channels(input: &[f32], in_ch: usize, out_ch: usize) -> Vec<f32> {
    if in_ch == out_ch || in_ch == 0 || out_ch == 0 {
        return input.to_vec();
    }
    let frames = input.len() / in_ch;
    let mut out = Vec::with_capacity(frames * out_ch);
    for frame in input.chunks(in_ch) {
        for c in 0..out_ch {
            out.push(*frame.get(c.min(in_ch - 1)).unwrap_or(&0.0));
        }
    }
    out
}

/// Naive linear interpolation. ponytail: rubato + quality constraints when
/// fase 2 (mobile) requires them.
fn resample_linear(input: &[f32], from: u32, to: u32, ch: usize) -> Vec<f32> {
    if from == to || input.is_empty() || ch == 0 {
        return input.to_vec();
    }
    let frames = input.len() / ch;
    let out_frames = (frames as u64 * u64::from(to) / u64::from(from)) as usize;
    let mut out = Vec::with_capacity(out_frames * ch);
    for i in 0..out_frames {
        let pos = i as f64 * f64::from(from) / f64::from(to);
        let i0 = (pos as usize).min(frames - 1);
        let i1 = (i0 + 1).min(frames - 1);
        let frac = (pos - i0 as f64) as f32;
        for c in 0..ch {
            let s0 = input[i0 * ch + c];
            let s1 = input[i1 * ch + c];
            out.push(s0 + (s1 - s0) * frac);
        }
    }
    out
}

/// Mutex: mpsc::Receiver is !Sync, the device callback needs Send. Contention
/// is impossible in practice (one callback thread); try_lock skips a tick.
type SharedChunkRx = Arc<std::sync::Mutex<mpsc::Receiver<(u64, Vec<f32>)>>>;

struct CallbackState {
    chunk_rx: SharedChunkRx,
    shared: Arc<Shared>,
}

impl Clone for CallbackState {
    fn clone(&self) -> Self {
        Self {
            chunk_rx: Arc::clone(&self.chunk_rx),
            shared: Arc::clone(&self.shared),
        }
    }
}

impl CallbackState {
    /// Drop stale-epoch chunks and pull whatever the decoder produced.
    fn refresh(&self, pending: &mut VecDeque<Vec<f32>>, last_epoch: &mut u64) {
        let epoch = self.shared.epoch.load(Ordering::Relaxed);
        if epoch != *last_epoch {
            pending.clear();
            self.shared.frames_this_epoch.store(0, Ordering::Relaxed);
            *last_epoch = epoch;
        }
        if let Ok(rx) = self.chunk_rx.try_lock() {
            while let Ok((e, chunk)) = rx.try_recv() {
                if e == epoch {
                    pending.push_back(chunk);
                }
            }
        }
    }
}

fn err_fn(err: cpal::Error) {
    // Callback errors (buffer underruns on idle streams) can fire per-callback;
    // print only the first so logs stay readable.
    use std::sync::atomic::{AtomicBool, Ordering};
    static SHOWN: AtomicBool = AtomicBool::new(false);
    if !SHOWN.swap(true, Ordering::Relaxed) {
        eprintln!("audio device error (further suppressed): {err}");
    }
}

pub struct CpalSymphoniaEngine {
    cmd_tx: mpsc::Sender<Cmd>,
    shared: Arc<Shared>,
    // Kept alive for the whole session; dropping it stops the device stream.
    _stream: cpal::Stream,
}

impl CpalSymphoniaEngine {
    pub fn new() -> Result<Self, AudioError> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| AudioError::Device("no output device".into()))?;
        let supported = device
            .default_output_config()
            .map_err(|e| AudioError::Device(e.to_string()))?;
        let config = supported.config();
        let device_rate = config.sample_rate;
        let device_channels = config.channels as usize;

        let shared = Arc::new(Shared {
            epoch: AtomicU64::new(0),
            base_ms: AtomicU64::new(0),
            frames_this_epoch: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            active: AtomicBool::new(false),
            volume_bits: AtomicU32::new(1.0f32.to_bits()),
            next_path: std::sync::Mutex::new(None),
            device_rate,
            device_channels,
        });

        let (chunk_tx, chunk_rx) = mpsc::sync_channel::<(u64, Vec<f32>)>(CHANNEL_DEPTH);
        let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
        thread::Builder::new()
            .name("audio-decode".into())
            .spawn({
                let shared = Arc::clone(&shared);
                move || decoder_loop(cmd_rx, chunk_tx, shared)
            })
            .map_err(|e| AudioError::Device(e.to_string()))?;

        let state = CallbackState {
            chunk_rx: Arc::new(std::sync::Mutex::new(chunk_rx)),
            shared: Arc::clone(&shared),
        };
        let stream = build_stream(&device, config, supported.sample_format(), state)?;
        stream
            .play()
            .map_err(|e| AudioError::Device(e.to_string()))?;
        Ok(Self {
            cmd_tx,
            shared,
            _stream: stream,
        })
    }

    fn swap(&self, session: Session) -> Result<(), AudioError> {
        let (tx, rx) = mpsc::sync_channel(0);
        self.cmd_tx
            .send(Cmd::Swap(session, tx))
            .map_err(|_| AudioError::Device("decoder thread gone".into()))?;
        rx.recv_timeout(Duration::from_secs(2))
            .map_err(|_| AudioError::Device("load timed out".into()))
    }
}

fn build_stream(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    state: CallbackState,
) -> Result<cpal::Stream, AudioError> {
    macro_rules! try_fmt {
        ($t:ty) => {{
            let state = state.clone();
            let channels = state.shared.device_channels;
            let mut pending: VecDeque<Vec<f32>> = VecDeque::new();
            let mut last_epoch = state.shared.epoch.load(Ordering::Relaxed);
            device
                .build_output_stream::<$t, _, _>(
                    config.clone(),
                    move |data: &mut [$t], _: &cpal::OutputCallbackInfo| {
                        fill_output(data, channels, &state, &mut pending, &mut last_epoch);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| AudioError::Device(e.to_string()))
        }};
    }
    match sample_format {
        cpal::SampleFormat::F32 => try_fmt!(f32),
        cpal::SampleFormat::I16 => try_fmt!(i16),
        cpal::SampleFormat::U8 => try_fmt!(u8),
        cpal::SampleFormat::I32 => try_fmt!(i32),
        other => Err(AudioError::Device(format!(
            "unsupported sample format {other:?}"
        ))),
    }
}

fn fill_output<T: cpal::SizedSample + cpal::FromSample<f32>>(
    data: &mut [T],
    channels: usize,
    state: &CallbackState,
    pending: &mut VecDeque<Vec<f32>>,
    last_epoch: &mut u64,
) {
    state.refresh(pending, last_epoch);
    let vol = f32::from_bits(state.shared.volume_bits.load(Ordering::Relaxed));
    let ch = channels.max(1);
    let mut played: u64 = 0;
    for frame in data.chunks_mut(ch) {
        let mut ok = false;
        if state.shared.playing.load(Ordering::Relaxed) {
            loop {
                if let Some(front) = pending.front_mut() {
                    if front.len() >= frame.len() {
                        let n = frame.len();
                        for (d, s) in frame.iter_mut().zip(front.drain(..n)) {
                            *d = T::from_sample(s * vol);
                        }
                        ok = true;
                        break;
                    }
                    pending.pop_front();
                    continue;
                }
                let Ok(rx) = state.chunk_rx.try_lock() else {
                    break;
                };
                match rx.try_recv() {
                    Ok((e, chunk)) if e == *last_epoch => pending.push_back(chunk),
                    Ok((e, _)) => {
                        // Stale epoch mid-frame: drop everything buffered.
                        pending.clear();
                        *last_epoch = e;
                    }
                    Err(_) => break, // underrun: silence
                }
            }
        }
        if ok {
            played += 1;
        } else {
            for d in frame {
                *d = T::from_sample(0.0);
            }
        }
    }
    state
        .shared
        .frames_this_epoch
        .fetch_add(played, Ordering::Relaxed);
}

impl AudioEngine for CpalSymphoniaEngine {
    fn load(&self, path: &FilePath) -> Result<(), AudioError> {
        let session = open_file(&path.0)?;
        self.swap(session)
    }

    fn play(&self) {
        self.shared.playing.store(true, Ordering::Relaxed);
    }

    fn pause(&self) {
        self.shared.playing.store(false, Ordering::Relaxed);
    }

    fn seek(&self, pos: Duration) -> Result<(), AudioError> {
        let ms = pos.as_millis() as u64;
        self.shared.base_ms.store(ms, Ordering::Relaxed);
        let (tx, rx) = mpsc::sync_channel(0);
        self.cmd_tx
            .send(Cmd::Seek { ms, done: tx })
            .map_err(|_| AudioError::Device("decoder thread gone".into()))?;
        rx.recv_timeout(Duration::from_secs(2))
            .map_err(|_| AudioError::Device("seek timed out".into()))
    }

    fn set_volume(&self, v: Volume) {
        self.shared
            .volume_bits
            .store(v.inner().to_bits(), Ordering::Relaxed);
    }

    fn enqueue_next(&self, path: &FilePath) {
        *self.shared.next_path.lock().unwrap() = Some(path.0.clone());
    }

    fn position(&self) -> Duration {
        Duration::from_millis(self.shared.position_ms())
    }

    fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
    }

    fn is_active(&self) -> bool {
        self.shared.active.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_up_and_down() {
        let out = resample_linear(&[0.0, 1.0, 2.0, 3.0], 2, 4, 2);
        assert_eq!(out.len(), 8); // 2 frames @2k → 4 frames @4k
        let out = resample_linear(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0], 4, 2, 2);
        assert_eq!(out.len(), 4); // 4 frames @4k → 2 frames @2k
    }

    #[test]
    fn channels_map() {
        // mono → stereo duplicates
        let out = map_channels(&[0.5, 0.25], 1, 2);
        assert_eq!(out, vec![0.5, 0.5, 0.25, 0.25]);
        // stereo → mono keeps left channel (truncation strategy)
        let out = map_channels(&[0.5, 0.9], 2, 1);
        assert_eq!(out, vec![0.5]);
    }
}
