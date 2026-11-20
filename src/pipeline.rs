use crate::config::snap_speed;
use gstreamer as gst;
use gstreamer::prelude::*;
use gtk4::gdk;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// 1 MiB read chunks for `filesrc` (256x fewer NVMe read() syscalls than the 4 KiB default,
/// allowing NVMe APST autonomous power state transitions to sleep between bursts).
const LOCAL_SSD_BLOCKSIZE_BYTES: u32 = 1_048_576;

#[derive(Debug, Clone)]
pub enum PlayerEvent {
    Error(String),
    Eos,
    Buffering(i32),
    VideoDimensions(u32, u32),
}

enum PipelineCmd {
    Open(String),
    SetPlaying(bool),
    SetRate(f64),
    Buffering(i32),
    AsyncDone,
    Seek(f64),
    SyncAudio,
    WakeUi,
    Stop,
    Shutdown,
}

struct WorkerShared {
    video_sink: gst::Element,
    events: Arc<Mutex<Vec<PlayerEvent>>>,
    on_event: Arc<Mutex<Option<Box<dyn Fn() + Send + Sync>>>>,
    event_wake_pending: Arc<AtomicBool>,
    want_playing: Arc<AtomicBool>,
    is_buffering: Arc<AtomicBool>,
    ui_visible: Arc<AtomicBool>,
    pos_ns: Arc<AtomicU64>,
    dur_ns: Arc<AtomicU64>,
    open_ready_ms: Arc<AtomicU64>,
    rate_bits: Arc<AtomicU64>,
    volume_bits: Arc<AtomicU64>,
    desired_muted: Arc<AtomicBool>,
    decoders: Arc<Mutex<(String, String)>>,
    caps_summary: Arc<Mutex<CapsSummary>>,
    active_vol: Arc<Mutex<Option<gst::Element>>>,
}

#[derive(Debug, Clone, Default)]
pub struct CapsSummary {
    pub memory_path: String,
    pub drm_format: String,
    pub resolution: String,
    pub width: u32,
    pub height: u32,
}

pub struct PlayerEngine {
    cmd_tx: mpsc::Sender<PipelineCmd>,
    shared: Arc<WorkerShared>,
    paintable: gdk::Paintable,
}

impl PlayerEngine {
    pub fn new(initial_volume: f64, initial_rate: f64) -> Result<Self, String> {
        let video_sink = gst::ElementFactory::make("gtk4paintablesink")
            .name("vp_vsink")
            .property("sync", true)
            .property("enable-last-sample", false)
            // Default 5ms is shorter than a single 60Hz/144Hz frame and causes spurious drops on
            // minor Wayland compositor jitter; 20ms matches standard GstVideoSink lateness budget.
            .property("max-lateness", 20_000_000i64)
            .build()
            .map_err(|e| format!("Failed to create gtk4paintablesink: {e}"))?;

        let paintable = video_sink.property::<gdk::Paintable>("paintable");
        let rate = snap_speed(initial_rate);
        let vol = if initial_volume.is_finite() {
            initial_volume.clamp(0.0, 1.5)
        } else {
            0.85
        };

        let shared = Arc::new(WorkerShared {
            video_sink,
            events: Arc::new(Mutex::new(Vec::new())),
            on_event: Arc::new(Mutex::new(None)),
            event_wake_pending: Arc::new(AtomicBool::new(false)),
            want_playing: Arc::new(AtomicBool::new(false)),
            is_buffering: Arc::new(AtomicBool::new(false)),
            ui_visible: Arc::new(AtomicBool::new(true)),
            pos_ns: Arc::new(AtomicU64::new(0)),
            dur_ns: Arc::new(AtomicU64::new(0)),
            open_ready_ms: Arc::new(AtomicU64::new(0)),
            rate_bits: Arc::new(AtomicU64::new(rate.to_bits())),
            volume_bits: Arc::new(AtomicU64::new(vol.to_bits())),
            desired_muted: Arc::new(AtomicBool::new(false)),
            decoders: Arc::new(Mutex::new(("idle".into(), "idle".into()))),
            caps_summary: Arc::new(Mutex::new(CapsSummary::default())),
            active_vol: Arc::new(Mutex::new(None)),
        });

        let (cmd_tx, cmd_rx) = mpsc::channel::<PipelineCmd>();
        let worker_shared = Arc::clone(&shared);
        let worker_cmd_tx = cmd_tx.clone();

        std::thread::Builder::new()
            .name("vp-gst-actor".into())
            .spawn(move || {
                run_pipeline_actor(cmd_rx, worker_cmd_tx, worker_shared);
            })
            .map_err(|e| format!("Failed to spawn GStreamer actor thread: {e}"))?;

        Ok(Self {
            cmd_tx,
            shared,
            paintable,
        })
    }

    pub fn paintable(&self) -> &gdk::Paintable {
        &self.paintable
    }

    pub fn video_sink(&self) -> &gst::Element {
        &self.shared.video_sink
    }

    pub fn decoders_snapshot(&self) -> (String, String) {
        self.shared
            .decoders
            .lock()
            .map(|g| g.clone())
            .unwrap_or_else(|_| ("idle".into(), "idle".into()))
    }

    pub fn caps_snapshot(&self) -> CapsSummary {
        self.shared
            .caps_summary
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    pub fn open_ready_ms(&self) -> u64 {
        self.shared.open_ready_ms.load(Ordering::Acquire)
    }

    pub fn set_event_callback<F>(&self, cb: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        if let Ok(mut slot) = self.shared.on_event.lock() {
            *slot = Some(Box::new(cb));
        }
    }

    pub fn drain_events(&self) -> Vec<PlayerEvent> {
        self.shared
            .event_wake_pending
            .store(false, Ordering::Release);
        if let Ok(mut q) = self.shared.events.lock() {
            std::mem::take(&mut *q)
        } else {
            Vec::new()
        }
    }

    pub fn open(&self, uri: &str) -> Result<(), String> {
        self.shared.want_playing.store(true, Ordering::Release);
        self.shared.is_buffering.store(false, Ordering::Release);
        self.shared.pos_ns.store(0, Ordering::Release);
        self.shared.dur_ns.store(0, Ordering::Release);
        self.shared.open_ready_ms.store(0, Ordering::Release);
        self.cmd_tx
            .send(PipelineCmd::Open(uri.to_string()))
            .map_err(|e| format!("Pipeline actor closed: {e}"))
    }

    pub fn stop(&self) {
        self.shared.want_playing.store(false, Ordering::Release);
        self.shared.is_buffering.store(false, Ordering::Release);
        let _ = self.cmd_tx.send(PipelineCmd::Stop);
    }

    pub fn set_playing(&self, playing: bool) {
        self.shared.want_playing.store(playing, Ordering::Release);
        let _ = self.cmd_tx.send(PipelineCmd::SetPlaying(playing));
    }

    pub fn is_playing(&self) -> bool {
        self.shared.want_playing.load(Ordering::Acquire)
    }

    /// Notify the actor whether the bottom seek bar is currently visible.
    /// When false, the actor stops 250ms position polling completely to save CPU wakeups.
    pub fn set_ui_visible(&self, visible: bool) {
        let prev = self.shared.ui_visible.swap(visible, Ordering::AcqRel);
        if visible && !prev {
            let _ = self.cmd_tx.send(PipelineCmd::WakeUi);
        }
    }

    pub fn set_rate(&self, rate: f64) {
        let snapped = snap_speed(rate);
        self.shared
            .rate_bits
            .store(snapped.to_bits(), Ordering::Release);
        let _ = self.cmd_tx.send(PipelineCmd::SetRate(snapped));
    }

    pub fn rate(&self) -> f64 {
        f64::from_bits(self.shared.rate_bits.load(Ordering::Acquire))
    }

    pub fn position_and_duration_secs(&self) -> (f64, f64) {
        let pos = self.shared.pos_ns.load(Ordering::Acquire) as f64 / 1e9;
        let dur = self.shared.dur_ns.load(Ordering::Acquire) as f64 / 1e9;
        (pos, dur)
    }

    pub fn seek_secs(&self, target_secs: f64) {
        let _ = self.cmd_tx.send(PipelineCmd::Seek(target_secs.max(0.0)));
    }

    pub fn set_volume(&self, volume: f64) {
        let clamped = if volume.is_finite() {
            volume.clamp(0.0, 1.5)
        } else {
            0.85
        };
        self.shared
            .volume_bits
            .store(clamped.to_bits(), Ordering::Release);
        let _ = self.cmd_tx.send(PipelineCmd::SyncAudio);
    }

    pub fn set_muted(&self, muted: bool) {
        self.shared.desired_muted.store(muted, Ordering::Release);
        let _ = self.cmd_tx.send(PipelineCmd::SyncAudio);
    }
}

impl Drop for PlayerEngine {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(PipelineCmd::Shutdown);
    }
}

fn run_pipeline_actor(
    cmd_rx: mpsc::Receiver<PipelineCmd>,
    cmd_tx: mpsc::Sender<PipelineCmd>,
    shared: Arc<WorkerShared>,
) {
    let mut active_pipe: Option<gst::Pipeline> = None;
    let mut current_rate = f64::from_bits(shared.rate_bits.load(Ordering::Acquire));
    let mut pending_rate_apply = false;
    let mut pending_seek: Option<f64> = None;
    let mut meta_resolved = false;
    let mut dims_emitted = false;
    let mut open_started_at: Option<Instant> = None;
    let mut last_vol = f64::from_bits(shared.volume_bits.load(Ordering::Acquire));
    let mut last_muted = shared.desired_muted.load(Ordering::Acquire);

    loop {
        // Sleep deeply (10s) when OSD is hidden and stream metadata is already resolved;
        // wake every 250ms only while the user can actually see the seek bar or during initial preroll.
        let poll_ms = if active_pipe.is_some() && shared.want_playing.load(Ordering::Acquire) {
            if shared.ui_visible.load(Ordering::Acquire) || !meta_resolved {
                250
            } else {
                10_000
            }
        } else {
            10_000
        };

        let first_cmd = match cmd_rx.recv_timeout(Duration::from_millis(poll_ms)) {
            Ok(c) => Some(c),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };

        let mut latest_open: Option<String> = None;
        let mut latest_seek: Option<f64> = None;
        let mut latest_play: Option<bool> = None;
        let mut latest_rate: Option<f64> = None;
        let mut latest_buf: Option<i32> = None;
        let mut got_async_done = false;
        let mut do_stop = false;
        let mut do_shutdown = false;

        let mut process_cmd = |cmd: PipelineCmd| match cmd {
            PipelineCmd::Open(uri) => {
                latest_open = Some(uri);
                latest_seek = None;
                do_stop = false;
            }
            PipelineCmd::Stop => {
                latest_open = None;
                latest_seek = None;
                do_stop = true;
            }
            PipelineCmd::SetPlaying(p) => latest_play = Some(p),
            PipelineCmd::SetRate(r) => latest_rate = Some(r),
            PipelineCmd::Buffering(pct) => latest_buf = Some(pct),
            PipelineCmd::AsyncDone => got_async_done = true,
            PipelineCmd::Seek(t) => latest_seek = Some(t),
            PipelineCmd::SyncAudio | PipelineCmd::WakeUi => {}
            PipelineCmd::Shutdown => do_shutdown = true,
        };

        if let Some(c) = first_cmd {
            process_cmd(c);
        }
        while let Ok(c) = cmd_rx.try_recv() {
            process_cmd(c);
        }

        if do_shutdown {
            teardown_pipeline(&mut active_pipe, &shared);
            break;
        }

        if do_stop {
            teardown_pipeline(&mut active_pipe, &shared);
            pending_rate_apply = false;
            pending_seek = None;
            meta_resolved = false;
            dims_emitted = false;
        }

        if let Some(uri) = latest_open {
            teardown_pipeline(&mut active_pipe, &shared);
            pending_rate_apply = (current_rate - 1.0).abs() > 0.01;
            pending_seek = None;
            meta_resolved = false;
            dims_emitted = false;
            open_started_at = Some(Instant::now());
            match build_and_start_pipeline(&uri, &cmd_tx, &shared) {
                Ok(pipe) => {
                    active_pipe = Some(pipe);
                }
                Err(err) => {
                    pending_rate_apply = false;
                    push_event(&shared, PlayerEvent::Error(err));
                }
            }
        }

        if got_async_done {
            if let Some(t0) = open_started_at.take() {
                let ms = t0.elapsed().as_millis().max(1) as u64;
                shared.open_ready_ms.store(ms, Ordering::Release);
            }
        }

        if let Some(pct) = latest_buf {
            let buff = pct < 100;
            shared.is_buffering.store(buff, Ordering::Release);
            if let Some(ref pipe) = active_pipe {
                let target = if !buff && shared.want_playing.load(Ordering::Acquire) {
                    gst::State::Playing
                } else {
                    gst::State::Paused
                };
                let _ = pipe.set_state(target);
            }
            push_event(&shared, PlayerEvent::Buffering(pct));
        }

        if let Some(play) = latest_play {
            if let Some(ref pipe) = active_pipe {
                let target = if play && !shared.is_buffering.load(Ordering::Acquire) {
                    gst::State::Playing
                } else {
                    gst::State::Paused
                };
                let _ = pipe.set_state(target);
            }
        }

        if let Some(new_rate) = latest_rate {
            current_rate = snap_speed(new_rate);
            if let Some(ref pipe) = active_pipe {
                if !apply_rate_seek(pipe, current_rate, None, &shared) {
                    pending_rate_apply = true;
                } else {
                    pending_rate_apply = false;
                }
            }
        } else if got_async_done && pending_rate_apply {
            if let Some(ref pipe) = active_pipe {
                if apply_rate_seek(pipe, current_rate, None, &shared) {
                    pending_rate_apply = false;
                }
            }
        }

        if let Some(target_secs) = latest_seek.or_else(|| {
            if got_async_done {
                pending_seek.take()
            } else {
                None
            }
        }) {
            if let Some(ref pipe) = active_pipe {
                if !apply_rate_seek(pipe, current_rate, Some(target_secs), &shared) {
                    pending_seek = Some(target_secs);
                }
            }
        }

        let dv = f64::from_bits(shared.volume_bits.load(Ordering::Acquire));
        let dm = shared.desired_muted.load(Ordering::Acquire);
        if let Ok(guard) = shared.active_vol.lock() {
            if let Some(ref vol) = *guard {
                if (dv - last_vol).abs() > 0.002 {
                    vol.set_property("volume", dv);
                    last_vol = dv;
                }
                if dm != last_muted {
                    vol.set_property("mute", dm);
                    last_muted = dm;
                }
            }
        }

        if let Some(ref pipe) = active_pipe {
            if shared.ui_visible.load(Ordering::Acquire) || got_async_done {
                if let Some(pos) = pipe.query_position::<gst::ClockTime>() {
                    shared.pos_ns.store(pos.nseconds(), Ordering::Release);
                }
            }
            if !meta_resolved || got_async_done {
                if let Some(dur) = pipe.query_duration::<gst::ClockTime>() {
                    shared.dur_ns.store(dur.nseconds(), Ordering::Release);
                }
                let (vdec, adec) = find_active_decoders(pipe);
                if let Ok(mut g) = shared.decoders.lock() {
                    if !vdec.is_empty() {
                        g.0 = vdec.clone();
                    }
                    if !adec.is_empty() {
                        g.1 = adec;
                    }
                }
                let (w, h) = update_caps_summary(&shared);
                if w > 0 && h > 0 && !dims_emitted {
                    dims_emitted = true;
                    push_event(&shared, PlayerEvent::VideoDimensions(w, h));
                }
                if !vdec.is_empty() && w > 0 && shared.dur_ns.load(Ordering::Acquire) > 0 {
                    meta_resolved = true;
                }
            }
        }
    }
}

/// Non-blocking 0ms seek dispatch: uses `current_state()` instead of waiting 50ms in `state()`,
/// and uses `KEY_UNIT | SNAP_NEAREST` for instant hardware keyframe scrubbing.
fn apply_rate_seek(
    pipe: &gst::Pipeline,
    rate: f64,
    explicit_secs: Option<f64>,
    shared: &WorkerShared,
) -> bool {
    let state = pipe.current_state();
    if matches!(state, gst::State::Null | gst::State::Ready) {
        return false;
    }
    let target = match explicit_secs {
        Some(s) => gst::ClockTime::from_nseconds((s.max(0.0) * 1e9) as u64),
        None => pipe
            .query_position::<gst::ClockTime>()
            .unwrap_or_else(|| gst::ClockTime::from_nseconds(shared.pos_ns.load(Ordering::Acquire))),
    };
    let flags = if explicit_secs.is_some() {
        gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT | gst::SeekFlags::SNAP_NEAREST
    } else {
        gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE
    };
    if pipe
        .seek(
            rate,
            flags,
            gst::SeekType::Set,
            target,
            gst::SeekType::None,
            gst::ClockTime::NONE,
        )
        .is_ok()
    {
        shared.pos_ns.store(target.nseconds(), Ordering::Release);
        true
    } else {
        false
    }
}

fn push_event(shared: &WorkerShared, ev: PlayerEvent) {
    if let Ok(mut q) = shared.events.lock() {
        q.push(ev);
    }
    if !shared.event_wake_pending.swap(true, Ordering::AcqRel) {
        if let Ok(cb) = shared.on_event.lock() {
            if let Some(ref f) = *cb {
                f();
            }
        }
    }
}

fn teardown_pipeline(active_pipe: &mut Option<gst::Pipeline>, shared: &WorkerShared) {
    if let Some(pipe) = active_pipe.take() {
        if let Some(bus) = pipe.bus() {
            bus.unset_sync_handler();
        }
        let _ = pipe.set_state(gst::State::Null);
        let _ = pipe.remove(&shared.video_sink);
        let _ = shared.video_sink.set_state(gst::State::Null);
    }
    if let Ok(mut v) = shared.active_vol.lock() {
        *v = None;
    }
    shared.pos_ns.store(0, Ordering::Release);
    shared.dur_ns.store(0, Ordering::Release);
    if let Ok(mut g) = shared.decoders.lock() {
        *g = ("idle".into(), "idle".into());
    }
    if let Ok(mut c) = shared.caps_summary.lock() {
        *c = CapsSummary::default();
    }
}

fn build_and_start_pipeline(
    uri: &str,
    cmd_tx: &mpsc::Sender<PipelineCmd>,
    shared: &Arc<WorkerShared>,
) -> Result<gst::Pipeline, String> {
    let pipe = gst::Pipeline::new();
    let is_local = uri.starts_with("file://");

    // Throttle software AV1 decoder (`dav1ddec`) before it enters PAUSED/PLAYING so it doesn't
    // spawn all 16 logical threads or buffer 16 frames (reduces CPU boost spikes & fan spin on AV1).
    pipe.connect_deep_element_added(|_bin, _sub_bin, elem| {
        if elem
            .factory()
            .is_some_and(|f| f.name().as_str() == "dav1ddec")
        {
            let on_battery = crate::telemetry::read_battery_info()
                .1
                .contains("Discharging");
            let threads: u32 = if on_battery { 4 } else { 6 };
            if elem.has_property("n-threads") {
                elem.set_property("n-threads", threads);
            }
            if elem.has_property("max-frame-delay") {
                elem.set_property("max-frame-delay", 2i64);
            }
        }
    });

    let vqueue = gst::ElementFactory::make("queue")
        .name("vqueue")
        .property("max-size-buffers", 3u32)
        .property("max-size-bytes", 0u32)
        .property("max-size-time", 0u64)
        .build()
        .map_err(|e| format!("Failed to create video queue: {e}"))?;

    pipe.add_many([&vqueue, &shared.video_sink])
        .map_err(|e| format!("Failed to add video sink branch: {e}"))?;
    gst::Element::link_many([&vqueue, &shared.video_sink])
        .map_err(|e| format!("Failed to link vqueue -> gtk4paintablesink: {e}"))?;

    let mut dec_builder = gst::ElementFactory::make("uridecodebin3")
        .name("vsrc")
        .property("uri", uri)
        .property("use-buffering", !is_local);
    if !is_local {
        dec_builder = dec_builder.property("buffer-duration", 8_000_000_000i64);
    }
    let vsrc = dec_builder
        .build()
        .map_err(|e| format!("Failed to create uridecodebin3: {e}"))?;

    // Optimize NVMe SSD active power on local files (1 MiB sequential read blocks instead of 4 KiB)
    // and enforce sane network timeouts on remote streams.
    vsrc.connect("source-setup", false, move |vals| {
        if let Ok(src) = vals[1].get::<gst::Element>() {
            if is_local {
                if src.has_property("blocksize") {
                    src.set_property("blocksize", LOCAL_SSD_BLOCKSIZE_BYTES);
                }
            } else {
                if src.has_property("timeout") {
                    src.set_property("timeout", 15u32);
                }
                if src.has_property("retries") {
                    src.set_property("retries", 2i32);
                }
            }
        }
        None
    });

    pipe.add(&vsrc)
        .map_err(|e| format!("Failed to add uridecodebin3: {e}"))?;

    let vqueue_weak = vqueue.downgrade();
    let pipe_weak = pipe.downgrade();
    let shared_for_pad = Arc::clone(shared);
    let audio_attached = Arc::new(AtomicBool::new(false));

    vsrc.connect_pad_added(move |_src, pad| {
        let caps = pad.current_caps().or_else(|| Some(pad.query_caps(None)));
        let is_video = caps
            .as_ref()
            .and_then(|c| c.structure(0))
            .map(|s| s.name().starts_with("video/"))
            .unwrap_or(false);
        let is_audio = caps
            .as_ref()
            .and_then(|c| c.structure(0))
            .map(|s| s.name().starts_with("audio/"))
            .unwrap_or(false);

        if is_video {
            if let Some(q) = vqueue_weak.upgrade() {
                if let Some(sink_pad) = q.static_pad("sink") {
                    if !sink_pad.is_linked() {
                        let _ = pad.link(&sink_pad);
                    }
                }
            }
        } else if is_audio && !audio_attached.swap(true, Ordering::AcqRel) {
            if let Some(parent_pipe) = pipe_weak.upgrade() {
                attach_audio_branch(&parent_pipe, pad, &shared_for_pad);
            }
        }
    });

    if let Some(bus) = pipe.bus() {
        let shared_bus = Arc::clone(shared);
        let cmd_tx = cmd_tx.clone();
        bus.set_sync_handler(move |_bus, msg| {
            match msg.view() {
                gst::MessageView::Error(err) => {
                    push_event(
                        &shared_bus,
                        PlayerEvent::Error(format!("{} ({:?})", err.error(), err.debug())),
                    );
                }
                gst::MessageView::Eos(_) => {
                    push_event(&shared_bus, PlayerEvent::Eos);
                }
                gst::MessageView::Buffering(buf) => {
                    let _ = cmd_tx.send(PipelineCmd::Buffering(buf.percent()));
                }
                gst::MessageView::AsyncDone(_) => {
                    let _ = cmd_tx.send(PipelineCmd::AsyncDone);
                }
                _ => {}
            }
            gst::BusSyncReply::Drop
        });
    }

    pipe.set_state(gst::State::Playing)
        .map_err(|e| format!("Failed to start GStreamer pipeline: {e}"))?;

    Ok(pipe)
}

fn attach_audio_branch(pipe: &gst::Pipeline, src_pad: &gst::Pad, shared: &WorkerShared) {
    let Ok(aqueue) = gst::ElementFactory::make("queue")
        .name("aqueue")
        .property("max-size-buffers", 32u32)
        .build()
    else {
        return;
    };
    let Ok(aconv) = gst::ElementFactory::make("audioconvert").build() else {
        return;
    };
    let scaletempo = gst::ElementFactory::make("scaletempo").build().ok();
    let Ok(ares) = gst::ElementFactory::make("audioresample").build() else {
        return;
    };

    let dv = f64::from_bits(shared.volume_bits.load(Ordering::Acquire));
    let dm = shared.desired_muted.load(Ordering::Acquire);
    let Ok(vol) = gst::ElementFactory::make("volume")
        .property("volume", dv)
        .property("mute", dm)
        .build()
    else {
        return;
    };
    let Ok(asink) = gst::ElementFactory::make("pipewiresink")
        .build()
        .or_else(|_| gst::ElementFactory::make("autoaudiosink").build())
    else {
        return;
    };
    if asink.has_property("enable-last-sample") {
        asink.set_property("enable-last-sample", false);
    }
    if asink.has_property("client-name") {
        asink.set_property("client-name", "Video Player");
    }
    // Hint PipeWire/WirePlumber that this is Movie playback so it selects power-saving audio quantums
    // (~21ms instead of 2.6ms low-latency IRQ wakeups).
    if asink.has_property("stream-properties") {
        let props = gst::Structure::builder("props")
            .field("media.type", "Audio")
            .field("media.category", "Playback")
            .field("media.role", "Movie")
            .build();
        asink.set_property("stream-properties", props);
    }

    let mut elems: Vec<&gst::Element> = vec![&aqueue, &aconv];
    if let Some(ref st) = scaletempo {
        elems.push(st);
    }
    elems.extend([&ares, &vol, &asink]);

    if pipe.add_many(elems.iter().copied()).is_err() {
        return;
    }
    if gst::Element::link_many(elems.iter().copied()).is_err() {
        return;
    }
    for e in elems.iter().rev() {
        let _ = e.sync_state_with_parent();
    }
    if let Some(sink_pad) = aqueue.static_pad("sink") {
        if !sink_pad.is_linked() {
            let _ = src_pad.link(&sink_pad);
        }
    }
    if let Ok(mut slot) = shared.active_vol.lock() {
        *slot = Some(vol);
    }
}

fn update_caps_summary(shared: &WorkerShared) -> (u32, u32) {
    let Some(pad) = shared.video_sink.static_pad("sink") else {
        return (0, 0);
    };
    let Some(caps) = pad.current_caps() else {
        return (0, 0);
    };
    let caps_str = caps.to_string();
    let is_dmabuf = caps_str.contains("memory:DMABuf");
    let memory_path = if is_dmabuf {
        "Zero-Copy DMA-BUF".into()
    } else if caps_str.contains("memory:GLMemory") {
        "GLMemory".into()
    } else if caps_str.contains("memory:VAMemory") {
        "VAMemory".into()
    } else {
        "SystemMemory (CPU Copy)".into()
    };

    let mut resolution = "-".into();
    let mut drm_format = "-".into();
    let mut width = 0u32;
    let mut height = 0u32;
    if let Some(s) = caps.structure(0) {
        let w = s.get::<i32>("width").unwrap_or(0);
        let h = s.get::<i32>("height").unwrap_or(0);
        if w > 0 && h > 0 {
            width = w as u32;
            height = h as u32;
            resolution = format!("{w}x{h}");
        }
        if let Ok(drm_fmt) = s.get::<&str>("drm-format") {
            drm_format = drm_fmt.to_string();
        } else if let Ok(fmt) = s.get::<&str>("format") {
            drm_format = fmt.to_string();
        }
    }

    if let Ok(mut c) = shared.caps_summary.lock() {
        c.memory_path = memory_path;
        c.drm_format = drm_format;
        c.resolution = resolution;
        c.width = width;
        c.height = height;
    }
    (width, height)
}

fn find_active_decoders(bin: &gst::Pipeline) -> (String, String) {
    let mut video_dec = String::new();
    let mut audio_dec = String::new();
    let mut stack: Vec<gst::Bin> = vec![bin.clone().upcast()];

    while let Some(cur) = stack.pop() {
        let iter = cur.iterate_elements();
        for elem in iter.into_iter().flatten() {
            if let Ok(sub_bin) = elem.clone().downcast::<gst::Bin>() {
                stack.push(sub_bin);
            }
            if let Some(factory) = elem.factory() {
                let klass = factory.metadata("klass").unwrap_or("");
                let name = factory.name().to_string();
                if klass.contains("Decoder") && klass.contains("Video") {
                    video_dec = name;
                } else if klass.contains("Decoder") && klass.contains("Audio") {
                    audio_dec = name;
                }
            }
        }
    }
    (video_dec, audio_dec)
}
