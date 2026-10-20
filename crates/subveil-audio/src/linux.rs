//! Linux-only PipeWire adapter. No fallback to input devices.
use crate::{Snapshot, pcm::PcmQueue};
use anyhow::{Context, bail};
use pipewire::{self as pw, properties::properties, spa};
use std::{
    cell::RefCell,
    collections::HashMap,
    io::Cursor,
    rc::Rc,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Default)]
struct Discovery {
    default: Option<String>,
    sinks: HashMap<u32, (u64, String)>,
    sink_revision: u64,
    metadata: HashMap<u32, (pw::metadata::MetadataListener, pw::metadata::Metadata)>,
    error: Option<String>,
}

impl Discovery {
    fn add_sink(&mut self, id: u32, name: &str) {
        // PipeWire may recycle object IDs before the next loop iteration.
        self.sink_revision = self.sink_revision.saturating_add(1);
        self.sinks.insert(id, (self.sink_revision, name.to_owned()));
    }

    fn target(&self) -> Option<(u32, u64, String)> {
        self.default.as_ref().and_then(|name| {
            self.sinks
                .iter()
                .find(|(_, (_, sink))| sink == name)
                .map(|(id, (revision, sink))| (*id, *revision, sink.clone()))
        })
    }
}

pub(crate) fn run(
    stop: &AtomicBool,
    shared: &Mutex<Snapshot>,
    stats: &mut Snapshot,
) -> anyhow::Result<()> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;
    let registry = core.get_registry_rc()?;
    let state = Rc::new(RefCell::new(Discovery::default()));
    let discovery = state.clone();
    let error_listener = core
        .add_listener_local()
        .error(move |_, _, _, message| {
            discovery.borrow_mut().error = Some(message.to_owned());
        })
        .register();
    let discovery = state.clone();
    let reg = registry.clone();
    let removed = state.clone();
    let registry_listener = registry
        .add_listener_local()
        .global(move |global| {
            let Some(props) = global.props.as_ref() else {
                return;
            };
            if props.get("media.class") == Some("Audio/Sink") {
                if let Some(name) = props.get("node.name") {
                    discovery.borrow_mut().add_sink(global.id, name);
                }
            } else if props.get("metadata.name") == Some("default") {
                match reg.bind::<pw::metadata::Metadata, _>(global) {
                    Ok(metadata) => {
                        let target = Rc::downgrade(&discovery);
                        let listener = metadata
                            .add_listener_local()
                            .property(move |subject, key, _, value| {
                                if subject == 0
                                    && (key == Some("default.audio.sink") || key.is_none())
                                    && let Some(target) = target.upgrade()
                                {
                                    target.borrow_mut().default = value.and_then(default_sink);
                                }
                                0
                            })
                            .register();
                        discovery
                            .borrow_mut()
                            .metadata
                            .insert(global.id, (listener, metadata));
                    }
                    Err(error) => discovery.borrow_mut().error = Some(format!("metadata: {error}")),
                }
            }
        })
        .global_remove(move |id| {
            let mut d = removed.borrow_mut();
            d.sinks.remove(&id);
            if d.metadata.remove(&id).is_some() {
                d.default = None;
            }
        })
        .register();
    let pcm = Rc::new(RefCell::new(PcmQueue::new()));
    // Preserve totals across service reconnection; each stream gets a new generation.
    {
        let mut q = pcm.borrow_mut();
        q.blocks = stats.blocks;
        q.samples = stats.samples;
        q.dropped_blocks = stats.dropped_blocks;
        q.dropped_samples = stats.dropped_samples;
        q.invalid = stats.invalid_chunks;
    }
    let mut active = None;
    let mut target = None;
    let mut last = Instant::now() - Duration::from_secs(1);
    let mut published = Instant::now() - Duration::from_secs(1);
    while !stop.load(Ordering::Relaxed) {
        if mainloop
            .loop_()
            .iterate(pw::loop_::Timeout::Finite(Duration::from_millis(20)))
            < 0
        {
            bail!("PipeWire loop failed");
        }
        if let Some(error) = state.borrow_mut().error.take() {
            bail!("{error}");
        }
        let next = state.borrow().target();
        if next != target {
            // Listener must be dropped before stream; destroy old links before replacement.
            active = None;
            pcm.borrow_mut().discard();
            stats.rms = 0.;
            stats.peak = 0.;
            stats.silent = true;
            if let Some((_, _, name)) = &next {
                let generation = stats.connections.saturating_add(1);
                active = Some(open_stream(
                    &core,
                    name,
                    stats.sample_rate,
                    generation,
                    pcm.clone(),
                    state.clone(),
                )?);
                stats.connections = generation;
            }
            target = next;
        }
        stats.device = target.as_ref().map(|(_, _, name)| name.clone());
        pcm.borrow_mut().drain(stats, stats.connections, &mut last);
        crate::expire_level(stats, last);
        stats.status = if active
            .as_ref()
            .is_some_and(|(_, stream)| matches!(stream.state(), pw::stream::StreamState::Streaming))
        {
            "正在捕获系统输出（仅本机）"
        } else if target.is_some() {
            "等待 monitor 流连接/运行"
        } else {
            "等待默认输出设备"
        }
        .into();
        if published.elapsed() >= Duration::from_millis(100) {
            crate::publish(shared, stats);
            published = Instant::now();
        }
    }
    drop(active);
    drop(registry_listener);
    drop(error_listener);
    // Release metadata listeners before their proxy objects.
    state.borrow_mut().metadata.clear();
    Ok(())
}

fn default_sink(value: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(value)
        .ok()?
        .get("name")?
        .as_str()
        .map(str::to_owned)
}

type Monitor<'a> = (
    pw::stream::StreamListener<Callback>,
    pw::stream::StreamBox<'a>,
);
struct Callback {
    pcm: Rc<RefCell<PcmQueue>>,
    rate: u32,
    generation: u64,
    valid: bool,
}

fn open_stream<'a>(
    core: &'a pw::core::CoreRc,
    name: &str,
    rate: u32,
    generation: u64,
    pcm: Rc<RefCell<PcmQueue>>,
    discovery: Rc<RefCell<Discovery>>,
) -> anyhow::Result<Monitor<'a>> {
    let stream = pw::stream::StreamBox::new(
        core,
        "Subveil output monitor",
        properties! {
            "application.name" => "Subveil",
            "media.type" => "Audio", "media.category" => "Capture", "media.role" => "Accessibility",
            "stream.capture.sink" => "true", "target.object" => name,
            "node.dont-fallback" => "true", "node.dont-reconnect" => "true",
            "resample.quality" => "4",
        },
    )?;
    let errors = discovery.clone();
    let listener = stream
        .add_local_listener_with_user_data(Callback {
            pcm,
            rate,
            generation,
            valid: false,
        })
        .state_changed(move |_, _, _, new| {
            if let pw::stream::StreamState::Error(error) = new {
                errors.borrow_mut().error = Some(format!("monitor: {error}"));
            }
        })
        .param_changed(move |_, data, id, param| {
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }
            data.valid = false;
            let Some(param) = param else {
                return;
            };
            let mut format = spa::param::audio::AudioInfoRaw::new();
            if format.parse(param).is_ok()
                && format.rate() == data.rate
                && format.channels() == 1
                && format.format() == spa::param::audio::AudioFormat::F32LE
            {
                data.valid = true;
            } else {
                discovery.borrow_mut().error = Some("unsupported negotiated PCM format".into());
            }
        })
        .process(|stream, data| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            if !data.valid {
                return;
            }
            let Some(plane) = buffer.datas_mut().first_mut() else {
                return;
            };
            let offset = plane.chunk().offset() as usize;
            let size = plane.chunk().size() as usize;
            let stride = plane.chunk().stride();
            let mut pcm = data.pcm.borrow_mut();
            if size == 0 {
                return;
            }
            if size > 262_144
                || plane
                    .chunk()
                    .flags()
                    .contains(spa::buffer::ChunkFlags::CORRUPTED)
            {
                pcm.invalid += 1;
                return;
            }
            // EMPTY has media-specific silence semantics; its payload may contain old bytes.
            if plane.chunk().flags().bits() & spa::sys::SPA_CHUNK_FLAG_EMPTY as i32 != 0 {
                pcm.push_silence(size, data.generation, data.rate);
                return;
            }
            if stride != 4 && stride != 0 {
                pcm.invalid += 1;
                return;
            }
            let Some(bytes) = plane.data() else {
                pcm.invalid += 1;
                return;
            };
            let Some(end) = offset.checked_add(size) else {
                pcm.invalid += 1;
                return;
            };
            let Some(bytes) = bytes.get(offset..end) else {
                pcm.invalid += 1;
                return;
            };
            pcm.push(bytes, data.generation, data.rate);
        })
        .register()?;
    let mut format = spa::param::audio::AudioInfoRaw::new();
    format.set_format(spa::param::audio::AudioFormat::F32LE);
    format.set_rate(rate);
    format.set_channels(1);
    let object = spa::pod::Object {
        type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: spa::param::ParamType::EnumFormat.as_raw(),
        properties: format.into(),
    };
    let bytes = spa::pod::serialize::PodSerializer::serialize(
        Cursor::new(Vec::new()),
        &spa::pod::Value::Object(object),
    )
    .map_err(|error| anyhow::anyhow!("serialize PCM format: {error:?}"))?
    .0
    .into_inner();
    let pod = spa::pod::Pod::from_bytes(&bytes).context("invalid serialized PCM format")?;
    stream.connect(
        spa::utils::Direction::Input,
        None,
        pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
        &mut [pod],
    )?;
    Ok((listener, stream))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_requires_a_valid_sink_name() {
        assert_eq!(
            default_sink(r#"{"name":"output.test"}"#).as_deref(),
            Some("output.test")
        );
        assert_eq!(default_sink("{}"), None);
        assert_eq!(default_sink("invalid"), None);
    }
    #[test]
    fn recycled_ids_and_names_still_require_a_new_stream() {
        let mut d = Discovery {
            default: Some("sink".into()),
            ..Discovery::default()
        };
        d.add_sink(42, "sink");
        let before = d.target();
        d.sinks.remove(&42);
        assert_eq!(d.target(), None);
        d.add_sink(42, "sink");
        assert_ne!(d.target(), before);
    }

    #[test]
    fn missing_default_never_falls_back_to_an_arbitrary_sink() {
        let mut d = Discovery {
            default: Some("unplugged".into()),
            ..Discovery::default()
        };
        d.add_sink(1, "other-output");
        assert_eq!(d.target(), None);
        d.default = None;
        assert_eq!(d.target(), None);
    }
}
