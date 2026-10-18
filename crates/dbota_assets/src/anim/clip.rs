//! The clips of a model's embedded animation (`CTRL.embedded_animation`: the `ANIM` block's
//! compressed frames, the `AGRP` block's decode key, the `ASEQ` block's sequences), sampled
//! per frame into glTF keyframes as Source2Viewer 20 wrote them for the asset cache.

use std::collections::{HashMap, HashSet};

use super::segment::{Attribute, BonePose, Segment};
use crate::error::{Result, malformed};
use crate::kv3::{self, Value};
use crate::model::{SourceBone, array, floats, multiply, rotate, source_skeleton, to_gltf};
use crate::resource::Resource;

/// Most bone-frames one call decodes (each becomes up to three keyframes); the largest
/// manifest model (a destruction animation) takes 41 k.
const MAX_BONE_FRAMES: usize = 1 << 21;
/// Most element reads, frame-block visits and segment bones one call goes through.
const MAX_WORK: usize = 1 << 28;
/// Most bone entries the decode key's channels may take (four channels of 150 bones here).
const MAX_REMAP: usize = 1 << 20;
/// Frame rates outside this are not a clip's (zero or less plays at one frame a second).
const MAX_FPS: f32 = 10_000.0;
const MIN_FPS: f32 = 1e-3;

/// An animation clip on a model's skeleton.
pub struct Clip {
    pub name: String,
    /// One per bone, in skeleton order.
    pub tracks: Vec<Track>,
}

/// A bone's keyframes in glTF's frame, linearly interpolated. A run of equal values keeps only
/// its ends, so a channel stops at its last change: a clip that holds its pose (Pudge's hook
/// throw) lasts until the last bone moves.
pub struct Track {
    pub rotation: Keys<4>,
    pub translation: Keys<3>,
    pub scale: Keys<3>,
}

#[derive(Default)]
pub struct Keys<const N: usize> {
    /// Seconds, increasing.
    pub times: Vec<f32>,
    pub values: Vec<[f32; N]>,
}

impl Clip {
    /// The time of its last keyframe.
    pub fn duration(&self) -> f32 {
        let last = |times: &[f32]| times.last().copied().unwrap_or(0.0);
        self.tracks
            .iter()
            .map(|t| {
                last(&t.rotation.times)
                    .max(last(&t.translation.times))
                    .max(last(&t.scale.times))
            })
            .fold(0.0, f32::max)
    }
}

/// The clips called `names` of the model `resource`, in the order the model lists them (its
/// sequences, then the animations no sequence names). A name the model lacks, and an additive
/// clip (deltas over a pose, which glTF has no way to say), is left out.
pub fn read_clips(resource: &Resource, names: &[&str]) -> Result<Vec<Clip>> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let ctrl = resource
        .kv3(b"CTRL")
        .ok_or_else(|| malformed("animation", "no CTRL"))??
        .root;
    let Some(embedded) = ctrl.get("embedded_animation") else {
        return Ok(Vec::new());
    };
    let data = resource
        .data()
        .ok_or_else(|| malformed("animation", "no DATA"))??;
    let bones = source_skeleton(&data)?;
    let group = block(resource, embedded, "group_data_block")?
        .ok_or_else(|| malformed("animation", "no group block"))?;
    let anim = block(resource, embedded, "anim_data_block")?
        .ok_or_else(|| malformed("animation", "no animation block"))?;
    let sequences = block(resource, embedded, "seqgroup_data_block")?;
    let channels = channels(group.get("m_decodeKey"), &bones)?;
    let descs = array(anim.get("m_animArray"));
    let mut segments = Segments {
        list: array(anim.get("m_segmentArray")),
        codecs: array(anim.get("m_decoderArray"))
            .iter()
            .map(|d| {
                d.get("m_szName")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            })
            .collect(),
        channels: &channels,
        built: HashMap::new(),
    };
    let mut budget = Budget {
        bone_frames: MAX_BONE_FRAMES,
        work: MAX_WORK,
    };
    let mut clips: Vec<Clip> = Vec::new();
    for (name, desc, sequence) in listing(sequences.as_ref(), descs) {
        if !names.contains(&name) || clips.iter().any(|c| c.name == name) {
            continue;
        }
        let flag = |v: Option<&Value>, key| {
            v.and_then(|v| v.get("m_flags")?.get(key)?.as_bool()) == Some(true)
        };
        let additive = flag(Some(desc), "m_bDelta")
            || flag(Some(desc), "m_bAnimGraphAdditive")
            || flag(sequence, "m_bLegacyDelta");
        if !additive {
            let tracks = decode(desc, &bones, &mut segments, &mut budget)?;
            clips.push(Clip {
                name: name.to_owned(),
                tracks,
            });
        }
    }
    Ok(clips)
}

/// The KV3 block `CTRL.embedded_animation.<key>` addresses; none for an index of zero or less.
fn block(resource: &Resource, embedded: &Value, key: &str) -> Result<Option<Value>> {
    let Some(index) = embedded.get(key).and_then(Value::as_i64) else {
        return Ok(None);
    };
    let Some(block) = usize::try_from(index)
        .ok()
        .filter(|&i| i > 0)
        .and_then(|i| resource.blocks().get(i))
    else {
        return Ok(None);
    };
    Ok(Some(kv3::decode(resource.bytes(block))?.root))
}

/// A decode-key channel: what it animates (`None`: nothing a skeleton shows) and, per bone,
/// the bone's element id in it.
struct Channel {
    attribute: Option<Attribute>,
    remap: Vec<Option<i64>>,
}

/// The decode key's channels; elements find their bone by name, ignoring case.
fn channels(key: Option<&Value>, bones: &[SourceBone]) -> Result<Vec<Channel>> {
    let key = key.ok_or_else(|| malformed("animation", "no decode key"))?;
    let count = array(key.get("m_dataChannelArray")).len();
    if count.saturating_mul(bones.len()) > MAX_REMAP {
        return Err(malformed(
            "animation",
            format!("{count} channels of {} bones", bones.len()),
        ));
    }
    let mut by_name = HashMap::with_capacity(bones.len());
    for (i, bone) in bones.iter().enumerate() {
        by_name.entry(bone.name.to_lowercase()).or_insert(i);
    }
    let mut out = Vec::new();
    for channel in array(key.get("m_dataChannelArray")) {
        let attribute = channel
            .get("m_szVariableName")
            .and_then(Value::as_str)
            .and_then(Attribute::from_name);
        let mut remap = vec![None; bones.len()];
        if matches!(attribute, Some(a) if a != Attribute::Data) {
            let names = array(channel.get("m_szElementNameArray"));
            let ids = array(channel.get("m_nElementIndexArray"));
            for (name, id) in names.iter().zip(ids) {
                let bone = name.as_str().and_then(|n| by_name.get(&n.to_lowercase()));
                if let (Some(&bone), Some(id)) = (bone, id.as_i64()) {
                    remap[bone] = Some(id);
                }
            }
        }
        out.push(Channel { attribute, remap });
    }
    Ok(out)
}

/// The model's clips as Source2Viewer lists them: per sequence (`ASEQ`), its name and the
/// animation its first fetch reference names; then every animation no sequence took the name
/// of. Without sequences, the animations themselves.
fn listing<'a>(
    sequences: Option<&'a Value>,
    descs: &'a [Value],
) -> Vec<(&'a str, &'a Value, Option<&'a Value>)> {
    let name = |v: &'a Value, key| v.get(key).and_then(Value::as_str).unwrap_or_default();
    let mut out = Vec::new();
    let mut taken = HashSet::new();
    if let Some(sequences) = sequences {
        let by_name: HashMap<&str, &Value> = descs.iter().map(|d| (name(d, "m_name"), d)).collect();
        let referenced = array(sequences.get("m_localSequenceNameArray"));
        for sequence in array(sequences.get("m_localS1SeqDescArray")) {
            let fetch = sequence.get("m_fetch");
            let first = array(fetch.and_then(|f| f.get("m_localReferenceArray")))
                .first()
                .and_then(Value::as_u64)
                .and_then(|i| referenced.get(usize::try_from(i).ok()?))
                .and_then(Value::as_str);
            if let Some(&desc) = first.and_then(|f| by_name.get(f)) {
                let own = name(sequence, "m_sName");
                taken.insert(own);
                out.push((own, desc, Some(sequence)));
            }
        }
    }
    for desc in descs {
        if !taken.contains(name(desc, "m_name")) {
            out.push((name(desc, "m_name"), desc, None));
        }
    }
    out
}

/// What decoding may still spend.
struct Budget {
    bone_frames: usize,
    work: usize,
}

impl Budget {
    fn spend(left: &mut usize, cost: usize, what: &str) -> Result<()> {
        *left = left
            .checked_sub(cost)
            .ok_or_else(|| malformed("animation", format!("more {what} than a model has")))?;
        Ok(())
    }
}

/// The animation block's segments, parsed when a clip first uses them.
struct Segments<'a> {
    list: &'a [Value],
    codecs: Vec<&'a str>,
    channels: &'a [Channel],
    built: HashMap<usize, Option<Segment<'a>>>,
}

impl<'a> Segments<'a> {
    fn get(&mut self, i: usize, budget: &mut Budget) -> Result<Option<&Segment<'a>>> {
        if !self.built.contains_key(&i) {
            let desc = self
                .list
                .get(i)
                .ok_or_else(|| malformed("animation", format!("no segment {i}")))?;
            let container = desc
                .get("m_container")
                .and_then(Value::as_blob)
                .ok_or_else(|| malformed("animation", format!("segment {i} has no data")))?;
            let channel = desc
                .get("m_nLocalChannel")
                .and_then(Value::as_u64)
                .and_then(|c| self.channels.get(usize::try_from(c).ok()?))
                .ok_or_else(|| malformed("animation", format!("segment {i} has no channel")))?;
            // Parsing looks at every element and every bone.
            let cost = container.len() / 2 + channel.remap.len();
            Budget::spend(&mut budget.work, cost, "segment bones")?;
            let segment =
                Segment::parse(container, &self.codecs, channel.attribute, &channel.remap)?;
            self.built.insert(i, segment);
        }
        Ok(self.built.get(&i).and_then(Option::as_ref))
    }
}

/// A run of frames and the segments that hold them (`m_frameblockArray`), ends included.
struct FrameBlock {
    start: usize,
    end: usize,
    segments: Vec<usize>,
}

/// Root motion up to a frame (`m_movementArray`), in Source's frame.
struct Movement {
    end: i64,
    angle: f32,
    position: [f32; 3],
}

/// Decodes animation `desc` frame by frame onto the skeleton and writes the keyframes.
fn decode(
    desc: &Value,
    bones: &[SourceBone],
    segments: &mut Segments,
    budget: &mut Budget,
) -> Result<Vec<Track>> {
    let data = desc.get("m_pData");
    let frames = data
        .and_then(|d| d.get("m_nFrames"))
        .and_then(Value::as_u64)
        .and_then(|f| usize::try_from(f).ok())
        .ok_or_else(|| malformed("animation", "no frame count"))?;
    Budget::spend(
        &mut budget.bone_frames,
        frames.max(1).saturating_mul(bones.len()),
        "frames",
    )?;
    let fps = frame_rate(desc)?;
    let blocks = frame_blocks(data)?;
    let movements = movements(desc)?;
    let mut pose: Vec<BonePose> = bones
        .iter()
        .map(|b| BonePose {
            position: b.position,
            rotation: b.rotation,
            scale: 1.0,
        })
        .collect();
    let mut writers: Vec<TrackWriter> = bones.iter().map(|_| TrackWriter::default()).collect();
    for frame in 0..frames {
        Budget::spend(&mut budget.work, blocks.len(), "frame blocks")?;
        for block in blocks.iter().filter(|b| (b.start..=b.end).contains(&frame)) {
            for &i in &block.segments {
                if let Some(segment) = segments.get(i, budget)? {
                    Budget::spend(&mut budget.work, segment.work(), "segment reads")?;
                    segment.read(frame - block.start, &mut pose)?;
                }
            }
        }
        let time = frame as f32 / fps;
        let previous = (frame as f32 - 1.0) / fps;
        let motion = (!movements.is_empty()).then(|| root_motion(&movements, frame));
        for ((bone, now), writer) in bones.iter().zip(&pose).zip(&mut writers) {
            let root = bone.parent.is_none();
            let (translation, rotation, scale) = local(now, motion.filter(|_| root), root);
            writer.rotation.submit(time, previous, rotation)?;
            writer.translation.submit(time, previous, translation)?;
            writer.scale.submit(time, previous, scale)?;
        }
    }
    if frames == 0 {
        for (bone, writer) in bones.iter().zip(&mut writers) {
            let (translation, rotation) =
                to_gltf(bone.position, bone.rotation, bone.parent.is_none());
            writer.rotation.keys.push(0.0, rotation);
            writer.translation.keys.push(0.0, translation);
            writer.scale.keys.push(0.0, [1.0; 3]);
        }
    }
    Ok(writers.into_iter().map(TrackWriter::finish).collect())
}

/// A bone's local transform in glTF's frame: the decoded pose, moved by the root motion on a
/// root bone. Non-finite values (only a damaged file has them) become zero.
fn local(pose: &BonePose, motion: Option<([f32; 3], f32)>, root: bool) -> Keyframe {
    let finite = |v: f32| if v.is_finite() { v } else { 0.0 };
    let (mut position, mut rotation) = (pose.position, pose.rotation);
    if let Some((offset, degrees)) = motion {
        let half = degrees.to_radians() * 0.5;
        let yaw = [0.0, 0.0, half.sin(), half.cos()];
        let moved = rotate(yaw, position);
        position = [0, 1, 2].map(|i| moved[i] + offset[i]);
        rotation = multiply(yaw, rotation);
    }
    let (translation, rotation) = to_gltf(position.map(finite), rotation.map(finite), root);
    (translation, rotation, [finite(pose.scale); 3])
}

type Keyframe = ([f32; 3], [f32; 4], [f32; 3]);

/// Frames a second; zero or less plays at one, as Source2Viewer does.
fn frame_rate(desc: &Value) -> Result<f32> {
    let fps = desc.get("fps").and_then(Value::as_f64).unwrap_or(0.0) as f32;
    if fps <= 0.0 {
        return Ok(1.0);
    }
    if !(MIN_FPS..=MAX_FPS).contains(&fps) {
        return Err(malformed("animation", format!("{fps} frames a second")));
    }
    Ok(fps)
}

fn frame_blocks(data: Option<&Value>) -> Result<Vec<FrameBlock>> {
    let index = |v: &Value, key| {
        v.get(key)
            .and_then(Value::as_u64)
            .and_then(|i| usize::try_from(i).ok())
    };
    array(data.and_then(|d| d.get("m_frameblockArray")))
        .iter()
        .map(|b| {
            let segments = array(b.get("m_segmentIndexArray"))
                .iter()
                .map(|s| s.as_u64().and_then(|s| usize::try_from(s).ok()))
                .collect::<Option<Vec<_>>>();
            match (index(b, "m_nStartFrame"), index(b, "m_nEndFrame"), segments) {
                (Some(start), Some(end), Some(segments)) => Ok(FrameBlock {
                    start,
                    end,
                    segments,
                }),
                _ => Err(malformed("animation", "a frame block without its range")),
            }
        })
        .collect()
}

fn movements(desc: &Value) -> Result<Vec<Movement>> {
    array(desc.get("m_movementArray"))
        .iter()
        .map(|m| {
            let end = m.get("endframe").and_then(Value::as_i64);
            let angle = m.get("angle").and_then(Value::as_f64);
            match (end, angle, floats::<3>(m.get("position"))) {
                (Some(end), Some(angle), Ok(position)) => Ok(Movement {
                    end,
                    angle: angle as f32,
                    position,
                }),
                _ => Err(malformed(
                    "animation",
                    "a movement without its end or offset",
                )),
            }
        })
        .collect()
}

/// The root's offset (planar) and yaw in degrees at `frame`: interpolated from the previous
/// movement's towards the one the frame is in, by the frame over that movement's end frame,
/// as Source2Viewer 20 does.
fn root_motion(movements: &[Movement], frame: usize) -> ([f32; 3], f32) {
    let frame = frame as i64;
    let index = movements
        .iter()
        .position(|m| m.end > frame)
        .unwrap_or(movements.len() - 1);
    let next = &movements[index];
    let (from, from_angle) = match index.checked_sub(1) {
        Some(i) => (movements[i].position, movements[i].angle),
        None => ([0.0; 3], 0.0),
    };
    let t = if next.end > 0 {
        frame as f32 / next.end as f32
    } else {
        1.0
    };
    let lerp = |a: f32, b: f32| a * (1.0 - t) + b * t;
    let position = [
        lerp(from[0], next.position[0]),
        lerp(from[1], next.position[1]),
        0.0,
    ];
    (position, lerp(from_angle, next.angle))
}

#[derive(Default)]
struct TrackWriter {
    rotation: KeyWriter<4>,
    translation: KeyWriter<3>,
    scale: KeyWriter<3>,
}

impl TrackWriter {
    fn finish(self) -> Track {
        Track {
            rotation: self.rotation.keys,
            translation: self.translation.keys,
            scale: self.scale.keys,
        }
    }
}

impl<const N: usize> Keys<N> {
    fn push(&mut self, time: f32, value: [f32; N]) {
        self.times.push(time);
        self.values.push(value);
    }
}

/// Writes a channel's keyframes, leaving out the inside of every run of equal values.
#[derive(Default)]
struct KeyWriter<const N: usize> {
    keys: Keys<N>,
    last: Option<[f32; N]>,
    skipped: bool,
}

impl<const N: usize> KeyWriter<N> {
    fn submit(&mut self, time: f32, previous: f32, value: [f32; N]) -> Result<()> {
        if self.last == Some(value) {
            self.skipped = true;
            return Ok(());
        }
        if let Some(last) = self.last.filter(|_| self.skipped) {
            // The run's end, so the interpolation starts from it and not from the run's start.
            self.skipped = false;
            self.keys.push(previous, last);
        }
        if self.keys.times.last().is_some_and(|&t| t >= time) {
            return Err(malformed("animation", "keyframe times do not increase"));
        }
        self.keys.push(time, value);
        self.last = Some(value);
        Ok(())
    }
}
