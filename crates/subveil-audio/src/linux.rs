//! Linux-only PipeWire adapter. No fallback to input devices.
use crate::{CaptureTarget, Snapshot, Source, SourceKind, pcm::PcmQueue};
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
    server: u32,
    sources: HashMap<u32, Source>,
    ports: HashMap<u32, Port>,
    default: Option<String>,
    sinks: HashMap<u32, (u64, String)>,
    sink_revision: u64,
    metadata: HashMap<u32, (pw::metadata::MetadataListener, pw::metadata::Metadata)>,
    error: Option<String>,
}
#[derive(Clone)]
struct Port {
    node: u32,
    output: bool,
    channel: String,
}
fn source(props: &spa::utils::dict::DictRef, server: u32) -> Option<Source> {
    let kind = match props.get("media.class")? {
        "Stream/Output/Audio" if props.get("application.name") != Some("Subveil") => {
            SourceKind::Playback
        }
        "Audio/Sink" => SourceKind::Output,
        "Audio/Source" => SourceKind::Input,
        _ => return None,
    };
    let serial = props.get("object.serial")?.parse().ok()?;
    let name = props
        .get("application.name")
        .or_else(|| props.get("node.description"))
        .or_else(|| props.get("node.name"))?;
    let stream = props.get("media.name").unwrap_or("");
    let label: String = format!("{name} {stream}")
        .chars()
        .filter(|c| !c.is_control())
        .take(256)
        .collect();
    Some(Source {
        server,
        serial,
        kind,
        label,
    })
}

pub(crate) fn sources() -> anyhow::Result<Vec<Source>> {
    use std::cell::Cell;
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;
    let registry = core.get_registry_rc()?;
    let result = Rc::new(RefCell::new(Vec::new()));
    let server = Rc::new(Cell::new(0));
    let cookie = server.clone();
    let errors = Rc::new(RefCell::new(None));
    let error = errors.clone();
    let complete = Rc::new(Cell::new(false));
    let done = complete.clone();
    let sequence = core.sync(0)?;
    let _core_listener = core
        .add_listener_local()
        .info(move |info| cookie.set(info.cookie()))
        .done(move |id, seq| {
            if id == pw::core::PW_ID_CORE && seq == sequence {
                done.set(true);
            }
        })
        .error(move |_, _, _, _| *error.borrow_mut() = Some("PipeWire 音源查询失败"))
        .register();
    let found = result.clone();
    let _registry_listener = registry
        .add_listener_local()
        .global(move |global| {
            if let Some(props) = global.props.as_ref()
                && let Some(item) = source(props, 0)
            {
                found.borrow_mut().push(item);
            }
        })
        .register();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !complete.get() {
        anyhow::ensure!(Instant::now() < deadline, "PipeWire 音源查询超时");
        mainloop
            .loop_()
            .iterate(pw::loop_::Timeout::Finite(Duration::from_millis(20)));
        if let Some(error) = errors.borrow_mut().take() {
            bail!("{error}");
        }
    }
    let mut items = result.borrow().clone();
    for item in &mut items {
        item.server = server.get();
    }
    items.sort_by_key(|item| item.picker_label());
    Ok(items)
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
    fn selected(&self, target: &CaptureTarget) -> Option<(u32, u64, String)> {
        match target {
            CaptureTarget::DefaultOutput => self.target(),
            CaptureTarget::Node(selected) => self
                .sources
                .iter()
                .find(|(_, source)| {
                    source.serial == selected.serial
                        && source.kind == selected.kind
                        && self.server == selected.server
                })
                .map(|(id, source)| (*id, source.serial, source.label.clone())),
        }
    }
}

pub(crate) fn run(
    stop: &AtomicBool,
    shared: &Mutex<Snapshot>,
    stats: &mut Snapshot,
    frames: Option<&crate::FrameReceiver>,
    selection: &CaptureTarget,
) -> anyhow::Result<()> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;
    let registry = core.get_registry_rc()?;
    let state = Rc::new(RefCell::new(Discovery::default()));
    let discovery = state.clone();
    let info = state.clone();
    let error_listener = core
        .add_listener_local()
        .info(move |data| info.borrow_mut().server = data.cookie())
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
            if let Some(item) = source(props, 0) {
                discovery.borrow_mut().sources.insert(global.id, item);
            }
            if let (Some(node), Some(direction)) =
                (props.get("node.id"), props.get("port.direction"))
                && let Ok(node) = node.parse()
            {
                discovery.borrow_mut().ports.insert(
                    global.id,
                    Port {
                        node,
                        output: direction == "out",
                        channel: props.get("audio.channel").unwrap_or("").into(),
                    },
                );
            }
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
            d.sources.remove(&id);
            d.ports.remove(&id);
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
    let mut links: Vec<pw::link::Link> = Vec::new();
    let mut linked_ports = Vec::<(u32, u32)>::new();
    let manual = matches!(
        selection,
        CaptureTarget::Node(Source {
            kind: SourceKind::Playback,
            ..
        })
    );
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
        let next = state.borrow().selected(selection);
        if next != target {
            // Listener must be dropped before stream; destroy old links before replacement.
            links.clear();
            linked_ports.clear();
            active = None;
            pcm.borrow_mut().discard();
            stats.rms = 0.;
            stats.peak = 0.;
            stats.silent = true;
            if let Some((_, serial, name)) = &next {
                let generation = stats.connections.saturating_add(1);
                active = Some(open_stream(
                    &core,
                    if matches!(selection, CaptureTarget::DefaultOutput) {
                        name.clone()
                    } else {
                        serial.to_string()
                    }
                    .as_str(),
                    stats.sample_rate,
                    generation,
                    pcm.clone(),
                    state.clone(),
                    match selection {
                        CaptureTarget::Node(source) => source.kind,
                        _ => SourceKind::Output,
                    },
                )?);
                stats.connections = generation;
            }
            target = next;
        }
        if manual && let (Some((source_id, _, _)), Some((_, stream))) = (&target, &active) {
            let discovery = state.borrow();
            let mut output: Vec<_> = discovery
                .ports
                .iter()
                .filter(|(_, p)| p.node == *source_id && p.output)
                .collect();
            let mut input: Vec<_> = discovery
                .ports
                .iter()
                .filter(|(_, p)| p.node == stream.node_id() && !p.output)
                .collect();
            output.sort_by_key(|(id, _)| **id);
            input.sort_by_key(|(id, _)| **id);
            anyhow::ensure!(
                output.len() <= 2,
                "所选播放流超过双声道；请使用已支持的单/双声道音源"
            );
            let pairs: Vec<_> = input
                .iter()
                .enumerate()
                .filter_map(|(index, (input_id, inp))| {
                    let source = output
                        .iter()
                        .find(|(_, out)| out.channel == inp.channel)
                        .or_else(|| output.get(if output.len() == 1 { 0 } else { index }));
                    source.map(|(out, _)| (**out, **input_id))
                })
                .collect();
            if pairs != linked_ports {
                links.clear();
                for (out, inp) in &pairs {
                    links.push(core.create_object::<pw::link::Link>("link-factory", &properties! {
                        "link.output.port" => out.to_string(), "link.input.port" => inp.to_string(),
                        "link.passive" => "true", "object.linger" => "false"
                    })?);
                }
                linked_ports = pairs;
            }
        }
        stats.device = target.as_ref().map(|(_, _, name)| name.clone());
        pcm.borrow_mut()
            .drain_to(stats, stats.connections, &mut last, frames);
        crate::expire_level(stats, last);
        stats.status = if active
            .as_ref()
            .is_some_and(|(_, stream)| matches!(stream.state(), pw::stream::StreamState::Streaming))
        {
            if manual {
                "正在捕获所选播放流"
            } else {
                "正在捕获所选音源"
            }
        } else if target.is_some() {
            "等待 monitor 流连接/运行"
        } else {
            if matches!(selection, CaptureTarget::DefaultOutput) {
                "等待默认输出设备"
            } else {
                "所选音源已断开；未回退到系统混音，请刷新并重新选择"
            }
        }
        .into();
        if published.elapsed() >= Duration::from_millis(100) {
            crate::publish(shared, stats);
            published = Instant::now();
        }
    }
    drop(links);
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
    channels: u32,
}

fn open_stream<'a>(
    core: &'a pw::core::CoreRc,
    name: &str,
    rate: u32,
    generation: u64,
    pcm: Rc<RefCell<PcmQueue>>,
    discovery: Rc<RefCell<Discovery>>,
    kind: SourceKind,
) -> anyhow::Result<Monitor<'a>> {
    let manual = kind == SourceKind::Playback;
    let monitor = kind != SourceKind::Input;
    let stream = pw::stream::StreamBox::new(
        core,
        "Subveil output monitor",
        properties! {
            "application.name" => "Subveil",
            "media.type" => "Audio", "media.category" => "Capture", "media.role" => "Accessibility",
            "stream.capture.sink" => if monitor {"true"} else {"false"}, "target.object" => name,
            "node.autoconnect" => if manual {"false"} else {"true"},
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
            channels: if manual { 2 } else { 1 },
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
                && format.channels() == data.channels
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
                pcm.push_silence(size / data.channels as usize, data.generation, data.rate);
                return;
            }
            if stride != (4 * data.channels) as i32 && stride != 0 {
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
            if data.channels == 2 {
                pcm.push_stereo(bytes, data.generation, data.rate);
            } else {
                pcm.push(bytes, data.generation, data.rate);
            }
        })
        .register()?;
    let mut format = spa::param::audio::AudioInfoRaw::new();
    format.set_format(spa::param::audio::AudioFormat::F32LE);
    format.set_rate(rate);
    format.set_channels(if manual { 2 } else { 1 });
    if manual {
        let mut positions = [0; spa::sys::SPA_AUDIO_MAX_CHANNELS as usize];
        positions[0] = spa::sys::SPA_AUDIO_CHANNEL_FL;
        positions[1] = spa::sys::SPA_AUDIO_CHANNEL_FR;
        format.set_position(positions);
    }
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
        if manual {
            pw::stream::StreamFlags::MAP_BUFFERS
        } else {
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS
        },
        &mut [pod],
    )?;
    Ok((listener, stream))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_stream_never_retargets_recycled_id_or_another_server() {
        let selected = Source {
            server: 7,
            serial: 42,
            kind: SourceKind::Playback,
            label: "one".into(),
        };
        let mut d = Discovery {
            server: 7,
            ..Default::default()
        };
        d.sources.insert(2, selected.clone());
        let target = CaptureTarget::Node(selected);
        assert!(d.selected(&target).is_some());
        if let Some(source) = d.sources.get_mut(&2) {
            source.serial = 43;
        }
        d.add_sink(3, "default");
        d.default = Some("default".into());
        assert!(d.selected(&target).is_none());
        if let Some(source) = d.sources.get_mut(&2) {
            source.serial = 42;
        }
        d.server = 8;
        assert!(d.selected(&target).is_none());
    }
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
