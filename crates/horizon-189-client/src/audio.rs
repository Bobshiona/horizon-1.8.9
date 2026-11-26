//! The 1.8.9 sound engine, `SoundHandler`/`SoundManager` over paulscode as
//! the game hears it: events from `sounds.json` (the launcher's asset index,
//! then each selected pack's, lowest first), a weighted pick among each
//! event's sounds, volume and pitch clamped as `getClampedVolume` and
//! `getClampedPitch` do, linear attenuation to 16 blocks (times the volume
//! above 1), panning from the listener's yaw and 28 voices at most. Ogg files
//! decode on a worker thread the first time they play and stay cached.
use horizon_assets_189::packs::Overlay;
use serde_json::Value;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
};

/// `SoundCategory`, in its order; the settings keep one volume each.
pub const CATEGORIES: [&str; 9] = [
    "master", "music", "record", "weather", "block", "hostile", "neutral", "player", "ambient",
];
/// paulscode's normal channels.
const VOICES: usize = 28;
/// The largest sound file read.
const FILE_LIMIT: u64 = 16 << 20;

/// `Block.SoundType`, as `afh.b` and its four subclasses define them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoundType {
    name: &'static str,
    pub volume: f32,
    pub pitch: f32,
}

impl SoundType {
    /// `getBreakSound`: `dig.<name>`, glass `dig.glass`, ladder `dig.wood`,
    /// anvil `dig.stone`, slime `mob.slime.big`.
    pub fn dig(self) -> String {
        match self.name {
            "glass" => "dig.glass".into(),
            "ladder" => "dig.wood".into(),
            "anvil" => "dig.stone".into(),
            "slime" => "mob.slime.big".into(),
            name => format!("dig.{name}"),
        }
    }

    /// `getStepSound`: `step.<name>`; glass steps like stone, slime is
    /// `mob.slime.small`.
    pub fn step(self) -> String {
        match self.name {
            "glass" => "step.stone".into(),
            "slime" => "mob.slime.small".into(),
            name => format!("step.{name}"),
        }
    }
}

/// Each block's `stepSound` by id, from `Block.registerBlocks` and the
/// block classes' constructors: the letter is the `afh` field (e and i
/// stone, f wood, g gravel, h grass, j metal, k glass, l cloth, m sand,
/// n snow, o ladder, p anvil, q slime).
const BLOCK_SOUNDS: &[u8; 198] = b"eihgifhieeeemgiiifhhkiiieeejjiehhiilehhhhjjiiihfiifljffeijfhgiiffojiffijfiiffinknlghiffimkkflffkfeiffjkfffhfiihhiiieeeeeeiikkfffeiieejfffeeiehhfipffffffjijeejiikhfffqejikhliikhfffeeiifffffffffffffff";

pub fn block_sound(id: u16) -> SoundType {
    let (name, volume, pitch) = match BLOCK_SOUNDS.get(usize::from(id)).copied() {
        Some(b'f') => ("wood", 1.0, 1.0),
        Some(b'g') => ("gravel", 1.0, 1.0),
        Some(b'h') => ("grass", 1.0, 1.0),
        Some(b'j') => ("stone", 1.0, 1.5),
        Some(b'k') => ("glass", 1.0, 1.0),
        Some(b'l') => ("cloth", 1.0, 1.0),
        Some(b'm') => ("sand", 1.0, 1.0),
        Some(b'n') => ("snow", 1.0, 1.0),
        Some(b'o') => ("ladder", 1.0, 1.0),
        Some(b'p') => ("anvil", 0.3, 1.0),
        Some(b'q') => ("slime", 1.0, 1.0),
        _ => ("stone", 1.0, 1.0),
    };
    SoundType {
        name,
        volume,
        pitch,
    }
}

/// `Material.isLiquid`: water and lava.
pub fn is_liquid(id: u16) -> bool {
    matches!(id, 8..=11)
}

#[derive(Clone, Debug, PartialEq)]
struct Entry {
    /// `sounds/<name>.ogg`, or another event's name when `event`.
    name: String,
    volume: f32,
    pitch: f32,
    weight: u32,
    event: bool,
    stream: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Event {
    category: usize,
    entries: Vec<Entry>,
}

/// One `sounds.json` merged into `events`, as `SoundHandler.loadSoundResource`
/// does: a new event or one marked `replace` starts over; otherwise its
/// sounds are added to what lower packs gave.
fn merge_events(json: &str, events: &mut HashMap<String, Event>) {
    let Ok(Value::Object(root)) = serde_json::from_str::<Value>(json) else {
        return;
    };
    for (name, definition) in root {
        let replace = definition
            .get("replace")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let category = definition
            .get("category")
            .and_then(Value::as_str)
            .and_then(|c| CATEGORIES.iter().position(|known| *known == c));
        let event = events.entry(name).or_default();
        if replace {
            *event = Event::default();
        }
        if let Some(category) = category
            && (replace || event.entries.is_empty())
        {
            event.category = category;
        }
        let Some(Value::Array(sounds)) = definition.get("sounds") else {
            continue;
        };
        for sound in sounds {
            let entry = match sound {
                Value::String(name) => Entry {
                    name: name.clone(),
                    volume: 1.0,
                    pitch: 1.0,
                    weight: 1,
                    event: false,
                    stream: false,
                },
                Value::Object(fields) => {
                    let Some(name) = fields.get("name").and_then(Value::as_str) else {
                        continue;
                    };
                    let number = |key: &str| fields.get(key).and_then(Value::as_f64);
                    Entry {
                        name: name.to_owned(),
                        volume: number("volume").map_or(1.0, |v| v as f32),
                        pitch: number("pitch").map_or(1.0, |v| v as f32),
                        weight: number("weight").map_or(1, |v| v.max(0.0) as u32),
                        event: fields.get("type").and_then(Value::as_str) == Some("event"),
                        stream: fields
                            .get("stream")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                    }
                }
                _ => continue,
            };
            event.entries.push(entry);
        }
    }
}

/// Decoded audio, mixed down to mono.
#[derive(Debug)]
pub struct Clip {
    samples: Vec<f32>,
    rate: u32,
}

fn decode(bytes: Vec<u8>) -> Option<Clip> {
    use symphonia::core::{
        audio::SampleBuffer, codecs::DecoderOptions, formats::FormatOptions, io::MediaSourceStream,
        meta::MetadataOptions, probe::Hint,
    };
    let stream = MediaSourceStream::new(Box::new(std::io::Cursor::new(bytes)), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("ogg");
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .ok()?;
    let mut format = probed.format;
    let track = format.default_track()?;
    let (id, rate) = (track.id, track.codec_params.sample_rate?);
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .ok()?;
    let mut samples = Vec::new();
    while let Ok(packet) = format.next_packet() {
        if packet.track_id() != id {
            continue;
        }
        let Ok(buffer) = decoder.decode(&packet) else {
            continue;
        };
        let spec = *buffer.spec();
        let channels = spec.channels.count().max(1);
        let mut interleaved = SampleBuffer::<f32>::new(buffer.capacity() as u64, spec);
        interleaved.copy_interleaved_ref(buffer);
        samples.extend(
            interleaved
                .samples()
                .chunks(channels)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32),
        );
    }
    (!samples.is_empty()).then_some(Clip { samples, rate })
}

/// Where sound files come from: the selected packs, then the launcher's
/// asset index (`assets/indexes/1.8.json` and its objects).
#[derive(Clone, Default)]
pub struct Sources {
    packs: Overlay,
    objects: HashMap<String, PathBuf>,
}

impl Sources {
    /// `assets` is the launcher's assets folder; `packs` the selected stack.
    pub fn open(assets: Option<&Path>, packs: &[PathBuf]) -> Self {
        let mut objects = HashMap::new();
        if let Some(assets) = assets
            && let Ok(text) = std::fs::read_to_string(assets.join("indexes/1.8.json"))
            && let Ok(Value::Object(root)) = serde_json::from_str::<Value>(&text)
            && let Some(Value::Object(index)) = root.get("objects")
        {
            for (name, object) in index {
                let Some(hash) = object.get("hash").and_then(Value::as_str) else {
                    continue;
                };
                if hash.len() > 2 && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                    let path = assets.join("objects").join(&hash[..2]).join(hash);
                    objects.insert(name.clone(), path);
                }
            }
        }
        Self {
            packs: Overlay::open(packs),
            objects,
        }
    }

    /// `minecraft/<name>`: a pack's copy, else the index's.
    fn read(&self, name: &str) -> Option<Vec<u8>> {
        if let Some(bytes) = self.packs.read(&format!("assets/{name}"), FILE_LIMIT) {
            return Some(bytes);
        }
        let path = self.objects.get(name)?;
        let size = std::fs::metadata(path).ok()?.len();
        (size <= FILE_LIMIT).then(|| std::fs::read(path).ok())?
    }

    fn events(&self) -> HashMap<String, Event> {
        let mut events = HashMap::new();
        if let Some(base) = self.objects.get("minecraft/sounds.json")
            && let Ok(text) = std::fs::read_to_string(base)
        {
            merge_events(&text, &mut events);
        }
        for bytes in self
            .packs
            .read_each("assets/minecraft/sounds.json", 4 << 20)
        {
            if let Ok(text) = std::str::from_utf8(&bytes) {
                merge_events(text, &mut events);
            }
        }
        events
    }
}

#[derive(Debug)]
struct Voice {
    clip: Arc<Clip>,
    /// `None` plays at the listener (`AttenuationType.NONE`).
    at: Option<[f64; 3]>,
    /// Linear attenuation's distance to silence.
    range: f64,
    gain: f32,
    /// Source samples per output sample (rate and pitch).
    step: f64,
    cursor: f64,
}

/// The listener and the playing voices, shared with the output callback.
#[derive(Debug, Default)]
pub struct Mixer {
    voices: Vec<Voice>,
    listener: [f64; 3],
    yaw: f32,
    rate: u32,
    /// The master level, applied live as paulscode's master volume.
    master: f32,
}

/// Left and right gains for a voice: linear attenuation (paulscode's
/// `ATTENUATION_LINEAR`), then a pan by how far to the side it is.
fn gains(voice: &Voice, listener: [f64; 3], yaw: f32) -> (f32, f32) {
    let Some(at) = voice.at else {
        return (voice.gain, voice.gain);
    };
    let d = [
        at[0] - listener[0],
        at[1] - listener[1],
        at[2] - listener[2],
    ];
    let distance = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let fade = (1.0 - distance / voice.range).clamp(0.0, 1.0) as f32;
    if fade <= 0.0 {
        return (0.0, 0.0);
    }
    // Facing +z at yaw 0, the right hand is -x.
    let (sin, cos) = yaw.to_radians().sin_cos();
    let pan = if distance > 1e-3 {
        ((-(cos as f64) * d[0] - (sin as f64) * d[2]) / distance) as f32
    } else {
        0.0
    };
    let gain = voice.gain * fade;
    (
        gain * (1.0 - pan).sqrt().min(1.0),
        gain * (1.0 + pan).sqrt().min(1.0),
    )
}

impl Mixer {
    fn push(&mut self, voice: Voice) {
        if self.voices.len() >= VOICES {
            // paulscode takes the oldest channel.
            self.voices.remove(0);
        }
        self.voices.push(voice);
    }

    /// Fill interleaved `out` with every voice; finished voices drop.
    pub fn mix(&mut self, out: &mut [f32], channels: usize) {
        out.fill(0.0);
        let channels = channels.max(1);
        let (listener, yaw) = (self.listener, self.yaw);
        for voice in &mut self.voices {
            let (left, right) = gains(voice, listener, yaw);
            let (left, right) = (left * self.master, right * self.master);
            let samples = &voice.clip.samples;
            let last = samples.len().saturating_sub(1) as f64;
            for frame in out.chunks_mut(channels) {
                if voice.cursor >= last {
                    break;
                }
                let at = voice.cursor as usize;
                let t = (voice.cursor - at as f64) as f32;
                let sample = samples[at] + (samples[at + 1] - samples[at]) * t;
                frame[0] += sample * left;
                if let Some(second) = frame.get_mut(1) {
                    *second += sample * right;
                }
                voice.cursor += voice.step;
            }
        }
        self.voices
            .retain(|voice| voice.cursor < voice.clip.samples.len().saturating_sub(1) as f64);
        for sample in out {
            *sample = sample.clamp(-1.0, 1.0);
        }
    }
}

/// A sound waiting for its file.
struct Request {
    file: String,
    at: Option<[f64; 3]>,
    range: f64,
    gain: f32,
    pitch: f32,
}

pub struct Audio {
    mixer: Arc<Mutex<Mixer>>,
    events: HashMap<String, Event>,
    cache: Arc<Mutex<HashMap<String, Option<Arc<Clip>>>>>,
    loads: Option<mpsc::Sender<Request>>,
    seed: u64,
    /// `GameSettings.getSoundLevel` per `CATEGORIES`.
    pub volumes: [f32; 9],
    /// Kept alive; `None` without an output device.
    _stream: Option<cpal::Stream>,
}

impl Audio {
    /// No output and no events: plays nothing.
    pub fn silent() -> Self {
        Self {
            mixer: Arc::new(Mutex::new(Mixer {
                master: 1.0,
                ..Mixer::default()
            })),
            events: HashMap::new(),
            cache: Arc::default(),
            loads: None,
            seed: 0x2545_f491_4f6c_dd1d,
            volumes: [1.0; 9],
            _stream: None,
        }
    }

    pub fn new(sources: Sources, volumes: [f32; 9]) -> Self {
        let mut audio = Self::silent();
        audio.set_volumes(volumes);
        let Some((stream, rate)) = open_output(audio.mixer.clone()) else {
            eprintln!("AUDIO_OUTPUT none");
            return audio;
        };
        if let Ok(mut mixer) = audio.mixer.lock() {
            mixer.rate = rate;
        }
        audio._stream = Some(stream);
        audio.events = sources.events();
        let (send, receive) = mpsc::channel::<Request>();
        let (mixer, cache) = (audio.mixer.clone(), audio.cache.clone());
        let spawned = std::thread::Builder::new()
            .name("Sound loader".into())
            .spawn(move || {
                crate::scheduling::apply("sound", crate::scheduling::Class::Utility);
                for request in receive {
                    let known = cache
                        .lock()
                        .ok()
                        .and_then(|c| c.get(&request.file).cloned());
                    let clip = match known {
                        Some(clip) => clip,
                        None => {
                            let clip = sources.read(&request.file).and_then(decode).map(Arc::new);
                            match &clip {
                                Some(clip) => eprintln!(
                                    "SOUND_LOADED {} samples={} rate={}",
                                    request.file,
                                    clip.samples.len(),
                                    clip.rate
                                ),
                                None => eprintln!("SOUND_MISSING {}", request.file),
                            }
                            if let Ok(mut cache) = cache.lock() {
                                cache.insert(request.file.clone(), clip.clone());
                            }
                            clip
                        }
                    };
                    if let (Some(clip), Ok(mut mixer)) = (clip, mixer.lock()) {
                        let voice = voice(clip, &request, mixer.rate);
                        mixer.push(voice);
                    }
                }
            });
        match spawned {
            Ok(_) => audio.loads = Some(send),
            Err(error) => eprintln!("AUDIO_LOADER_FAILED {error}"),
        }
        eprintln!("AUDIO_OUTPUT rate={rate} events={}", audio.events.len());
        audio
    }

    /// New levels: master at once for everything playing, the categories
    /// for sounds started from now on.
    pub fn set_volumes(&mut self, volumes: [f32; 9]) {
        self.volumes = volumes;
        if let Ok(mut mixer) = self.mixer.lock() {
            mixer.master = volumes[0];
        }
    }

    /// Where the camera is and which way it faces.
    pub fn set_listener(&self, position: [f64; 3], yaw: f32) {
        if let Ok(mut mixer) = self.mixer.lock() {
            mixer.listener = position;
            mixer.yaw = yaw;
        }
    }

    pub fn stop_all(&self) {
        if let Ok(mut mixer) = self.mixer.lock() {
            mixer.voices.clear();
        }
    }

    fn random(&mut self, below: u32) -> u32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        if below == 0 {
            0
        } else {
            (self.seed % u64::from(below)) as u32
        }
    }

    /// `SoundEventAccessorComposite.cloneEntry`: a weighted pick, following
    /// event references.
    fn pick(&mut self, name: &str, depth: u8) -> Option<(Entry, usize)> {
        let event = self.events.get(name)?.clone();
        let total: u32 = event.entries.iter().map(|e| e.weight).sum();
        let mut roll = self.random(total);
        for entry in &event.entries {
            if roll < entry.weight {
                if entry.event {
                    return (depth < 8)
                        .then(|| self.pick(&entry.name, depth + 1))
                        .flatten()
                        .map(|(e, _)| (e, event.category));
                }
                return Some((entry.clone(), event.category));
            }
            roll -= entry.weight;
        }
        None
    }

    /// `SoundManager.playSound`: `name` is an event (`random.bow`), `at`
    /// `None` for interface sounds.
    pub fn play(&mut self, name: &str, at: Option<[f64; 3]>, volume: f32, pitch: f32) {
        let Some(loads) = self.loads.clone() else {
            return;
        };
        let name = name.strip_prefix("minecraft:").unwrap_or(name);
        let Some((entry, category)) = self.pick(name, 0) else {
            return;
        };
        // Music and records stream; this engine plays effects.
        if entry.stream {
            return;
        }
        // The master level applies in the mixer.
        let level = if category == 0 {
            1.0
        } else {
            self.volumes[category]
        };
        if level * self.volumes[0] <= 0.0 {
            return;
        }
        let gain = (volume * entry.volume * level).clamp(0.0, 1.0);
        let request = Request {
            file: format!("minecraft/sounds/{}.ogg", entry.name),
            at,
            range: 16.0 * f64::from(volume.max(1.0)),
            gain,
            pitch: (pitch * entry.pitch).clamp(0.5, 2.0),
        };
        let cached = self
            .cache
            .lock()
            .ok()
            .and_then(|cache| cache.get(&request.file).cloned());
        match cached {
            Some(Some(clip)) => {
                if let Ok(mut mixer) = self.mixer.lock() {
                    let voice = voice(clip, &request, mixer.rate);
                    mixer.push(voice);
                }
            }
            Some(None) => {}
            None => {
                let _ = loads.send(request);
            }
        }
    }
}

fn voice(clip: Arc<Clip>, request: &Request, rate: u32) -> Voice {
    let step = f64::from(clip.rate) * f64::from(request.pitch) / f64::from(rate.max(1));
    Voice {
        clip,
        at: request.at,
        range: request.range,
        gain: request.gain,
        step,
        cursor: 0.0,
    }
}

/// The default output device's stream, mixing `mixer`.
fn open_output(mixer: Arc<Mutex<Mixer>>) -> Option<(cpal::Stream, u32)> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let device = cpal::default_host().default_output_device()?;
    let config = device.default_output_config().ok()?;
    let rate = config.sample_rate().0;
    let channels = usize::from(config.channels());
    let stream_config = config.config();
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => build::<f32>(&device, &stream_config, channels, mixer),
        cpal::SampleFormat::I16 => build::<i16>(&device, &stream_config, channels, mixer),
        cpal::SampleFormat::U16 => build::<u16>(&device, &stream_config, channels, mixer),
        _ => None,
    }?;
    stream.play().ok()?;
    Some((stream, rate))
}

fn build<T: cpal::SizedSample + cpal::FromSample<f32>>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    mixer: Arc<Mutex<Mixer>>,
) -> Option<cpal::Stream> {
    use cpal::traits::DeviceTrait;
    let mut scratch = Vec::new();
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                scratch.resize(data.len(), 0.0f32);
                match mixer.lock() {
                    Ok(mut mixer) => mixer.mix(&mut scratch, channels),
                    Err(_) => scratch.fill(0.0),
                }
                for (out, sample) in data.iter_mut().zip(&scratch) {
                    *out = T::from_sample(*sample);
                }
            },
            |error| eprintln!("AUDIO_STREAM_ERROR {error}"),
            None,
        )
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sounds_json_merges_like_load_sound_resource() {
        let mut events = HashMap::new();
        merge_events(
            r#"{"random.bow":{"category":"neutral","sounds":["random/bow"]},
                "step.stone":{"category":"block","sounds":["step/stone1",
                    {"name":"step/stone2","volume":0.5,"pitch":1.2,"weight":3}]},
                "mob.alias":{"category":"hostile","sounds":[{"name":"random.bow","type":"event"}]}}"#,
            &mut events,
        );
        assert_eq!(events["random.bow"].category, 6);
        let stone = &events["step.stone"];
        assert_eq!(stone.category, 4);
        assert_eq!(stone.entries.len(), 2);
        assert_eq!(
            (
                stone.entries[1].volume,
                stone.entries[1].pitch,
                stone.entries[1].weight
            ),
            (0.5, 1.2, 3)
        );
        assert!(events["mob.alias"].entries[0].event);
        // A pack adds to an event, or replaces it.
        merge_events(
            r#"{"random.bow":{"sounds":["custom/bow"]},
                "step.stone":{"replace":true,"category":"player","sounds":["custom/step"]}}"#,
            &mut events,
        );
        assert_eq!(events["random.bow"].entries.len(), 2);
        assert_eq!(events["random.bow"].category, 6, "category stays");
        assert_eq!(events["step.stone"].entries.len(), 1);
        assert_eq!(events["step.stone"].category, 7);
        merge_events("not json", &mut events);
        assert_eq!(events.len(), 3);
    }

    #[test]
    fn block_sound_types_follow_register_blocks() {
        let names = |id| {
            let t = block_sound(id);
            (t.step(), t.dig(), t.volume, t.pitch)
        };
        let s = |a: &str, b: &str, v, p| (a.to_owned(), b.to_owned(), v, p);
        assert_eq!(names(1), s("step.stone", "dig.stone", 1.0, 1.0));
        assert_eq!(names(2), s("step.grass", "dig.grass", 1.0, 1.0));
        assert_eq!(names(20), s("step.stone", "dig.glass", 1.0, 1.0));
        assert_eq!(names(35), s("step.cloth", "dig.cloth", 1.0, 1.0));
        assert_eq!(names(42), s("step.stone", "dig.stone", 1.0, 1.5));
        assert_eq!(names(65), s("step.ladder", "dig.wood", 1.0, 1.0));
        assert_eq!(names(145), s("step.anvil", "dig.stone", 0.3, 1.0));
        assert_eq!(names(165), s("mob.slime.small", "mob.slime.big", 1.0, 1.0));
        assert_eq!(names(5).0, "step.wood");
        assert_eq!(names(12).0, "step.sand");
        assert_eq!(names(13).0, "step.gravel");
        assert_eq!(names(78).0, "step.snow");
        assert_eq!(names(400).0, "step.stone");
    }

    fn clip(samples: Vec<f32>) -> Arc<Clip> {
        Arc::new(Clip {
            samples,
            rate: 44_100,
        })
    }

    #[test]
    fn voices_fade_linearly_and_pan_by_yaw() {
        let voice = |at| Voice {
            clip: clip(vec![1.0; 4]),
            at: Some(at),
            range: 16.0,
            gain: 1.0,
            step: 1.0,
            cursor: 0.0,
        };
        // Ahead at yaw 0 (+z): both sides, half way faded by 8 blocks.
        let (l, r) = gains(&voice([0.0, 0.0, 8.0]), [0.0; 3], 0.0);
        assert!((l - 0.5).abs() < 1e-4 && (r - 0.5).abs() < 1e-4);
        // At -x the right hand side: right only.
        let (l, r) = gains(&voice([-4.0, 0.0, 0.0]), [0.0; 3], 0.0);
        assert!(l < 1e-4 && (r - 0.75).abs() < 1e-4);
        // Turned to face -x (yaw 90), the same source is ahead.
        let (l, r) = gains(&voice([-4.0, 0.0, 0.0]), [0.0; 3], 90.0);
        assert!((l - r).abs() < 1e-3);
        // Out of range is silent; an interface sound is not attenuated.
        assert_eq!(gains(&voice([0.0, 0.0, 20.0]), [0.0; 3], 0.0), (0.0, 0.0));
        let mut flat = voice([0.0; 3]);
        flat.at = None;
        flat.gain = 0.3;
        assert_eq!(gains(&flat, [50.0; 3], 0.0), (0.3, 0.3));
    }

    #[test]
    fn the_mixer_resamples_drops_finished_voices_and_keeps_28() {
        let mut mixer = Mixer {
            rate: 44_100,
            master: 1.0,
            ..Mixer::default()
        };
        let request = Request {
            file: String::new(),
            at: None,
            range: 16.0,
            gain: 1.0,
            pitch: 2.0,
        };
        // Pitch 2 steps two samples a frame: 0, 2, 4.
        mixer.push(voice(
            clip((0..6).map(|i| i as f32 / 10.0).collect()),
            &request,
            44_100,
        ));
        let mut out = vec![0.0; 8];
        mixer.mix(&mut out, 2);
        assert_eq!(&out[..6], &[0.0, 0.0, 0.2, 0.2, 0.4, 0.4]);
        assert_eq!(&out[6..], &[0.0, 0.0]);
        assert!(mixer.voices.is_empty());
        for _ in 0..40 {
            mixer.push(voice(clip(vec![0.0; 100]), &request, 44_100));
        }
        assert_eq!(mixer.voices.len(), VOICES);
    }
}
