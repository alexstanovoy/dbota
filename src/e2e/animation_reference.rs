//! Independent reference: subtract the explicit movement baked into a raw clip.
use heatel::{
    Resource,
    anim::{Clip, Keys},
    kv3::Value,
};

pub struct Trajectory {
    frames: u32,
    fps: f32,
    points: Vec<(u32, [f32; 3], f32)>,
    pub animation: String,
}

impl Trajectory {
    pub fn read(resource: &Resource, name: &str) -> Self {
        let ctrl = resource
            .kv3(b"CTRL")
            .expect("CTRL")
            .expect("CTRL data")
            .root;
        let embedded = ctrl.get("embedded_animation").expect("embedded animations");
        let anim = block(resource, embedded, "anim_data_block");
        let sequences = block(resource, embedded, "seqgroup_data_block");
        let fetched = array(&sequences, "m_localS1SeqDescArray")
            .iter()
            .find(|sequence| sequence.get("m_sName").and_then(Value::as_str) == Some(name))
            .and_then(|sequence| sequence.get("m_fetch"))
            .and_then(|fetch| array(fetch, "m_localReferenceArray").first())
            .and_then(Value::as_u64)
            .and_then(|index| array(&sequences, "m_localSequenceNameArray").get(index as usize))
            .and_then(Value::as_str)
            .unwrap_or(name);
        let desc = array(&anim, "m_animArray")
            .iter()
            .find(|desc| desc.get("m_name").and_then(Value::as_str) == Some(fetched))
            .or_else(|| {
                array(&anim, "m_animArray")
                    .iter()
                    .find(|desc| desc.get("m_name").and_then(Value::as_str) == Some(name))
            })
            .expect("animation descriptor");
        let frames = desc
            .get("m_pData")
            .and_then(|data| data.get("m_nFrames"))
            .and_then(Value::as_u64)
            .expect("frame count") as u32;
        let fps = desc.get("fps").and_then(Value::as_f64).expect("frame rate") as f32;
        let points = array(desc, "m_movementArray")
            .iter()
            .map(|movement| {
                let end = movement
                    .get("endframe")
                    .and_then(Value::as_u64)
                    .expect("movement end");
                let position = array(movement, "position");
                let position =
                    std::array::from_fn(|axis| position[axis].as_f64().expect("position") as f32);
                let angle = movement
                    .get("angle")
                    .and_then(Value::as_f64)
                    .expect("movement yaw") as f32;
                (end as u32, position, angle)
            })
            .collect();
        let animation = desc
            .get("m_name")
            .and_then(Value::as_str)
            .expect("animation name")
            .to_owned();
        Self {
            frames,
            fps: fps.max(1e-3),
            points,
            animation,
        }
    }

    pub fn times(&self) -> impl Iterator<Item = f32> + '_ {
        (0..self.frames).map(|frame| frame as f32 / self.fps)
    }

    pub fn pose(&self, raw: &Clip, joint: usize, time: f32) -> ([f32; 3], [f32; 4]) {
        let position = sample(&raw.tracks[joint].translation, time, false);
        let rotation = sample(&raw.tracks[joint].rotation, time, true);
        let (offset, yaw) = self.movement((time * self.fps).round() as u32);
        // Source (x,y,z) -> glTF (y,z,x), inches -> metres. Root movement is
        // planar, so its authored Z is not part of the trajectory added by the raw reader.
        let dx = position[0] - offset[1] * 0.0254;
        let dy = position[1];
        let dz = position[2] - offset[0] * 0.0254;
        let (s, c) = yaw.to_radians().sin_cos();
        let position = [c * dx - s * dz, dy, s * dx + c * dz];
        let (s, c) = (yaw.to_radians() * 0.5).sin_cos();
        let [x, y, z, w] = rotation;
        // Remove world-up yaw on the left; the complete authored rotation remains.
        (
            position,
            [c * x - s * z, c * y - s * w, c * z + s * x, c * w + s * y],
        )
    }

    fn movement(&self, frame: u32) -> ([f32; 3], f32) {
        if self.points.is_empty() {
            return ([0.0; 3], 0.0);
        }
        let index = self
            .points
            .iter()
            .position(|point| point.0 > frame)
            .unwrap_or(self.points.len() - 1);
        let (end, position, angle) = self.points[index];
        let (previous, previous_angle) = if index == 0 {
            ([0.0; 3], 0.0)
        } else {
            (self.points[index - 1].1, self.points[index - 1].2)
        };
        let share = if end == 0 {
            1.0
        } else {
            frame as f32 / end as f32
        };
        (
            std::array::from_fn(|axis| previous[axis] + (position[axis] - previous[axis]) * share),
            previous_angle + (angle - previous_angle) * share,
        )
    }
}

pub fn sample<const N: usize>(keys: &Keys<N>, time: f32, rotation: bool) -> [f32; N] {
    let next = keys.times.partition_point(|key| *key <= time);
    if next == 0 {
        return keys.values[0];
    }
    if next == keys.times.len() {
        return *keys.values.last().expect("last key");
    }
    let a = keys.values[next - 1];
    let b = keys.values[next];
    let share = (time - keys.times[next - 1]) / (keys.times[next] - keys.times[next - 1]);
    let sign = if rotation && a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>() < 0.0 {
        -1.0
    } else {
        1.0
    };
    let mut value = std::array::from_fn(|axis| a[axis] + (sign * b[axis] - a[axis]) * share);
    if rotation {
        let length = value.iter().map(|x| x * x).sum::<f32>().sqrt();
        for component in &mut value {
            *component /= length;
        }
    }
    value
}

fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value.get(key).and_then(Value::as_array).unwrap_or(&[])
}

fn block(resource: &Resource, embedded: &Value, key: &str) -> Value {
    let Some(index) = embedded
        .get(key)
        .and_then(Value::as_u64)
        .filter(|index| *index > 0)
    else {
        return Value::Null;
    };
    let block = &resource.blocks()[index as usize];
    heatel::kv3::decode(resource.bytes(block))
        .expect("embedded KV3")
        .root
}
