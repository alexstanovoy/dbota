//! Replays in a browser page: the page's script (web/replays.js) stores them in the Origin
//! Private File System, or IndexedDB where that cannot be written, and publishes its calls as
//! `globalThis.dbota_replays`. Every call is a promise; the answers reach the library on the
//! channel it polls.

use std::sync::mpsc::Sender;

use js_sys::{Array, Function, Promise, Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::{JsFuture, spawn_local};

use super::backend::{Backend, Done, Op};
use super::index::{MAX_BYTES, Record};

/// The calls the page published.
struct Browser {
    api: JsValue,
}

/// The backend for this page: the one its script published, or one that says there is none.
pub fn open() -> Box<dyn Backend> {
    let api = Reflect::get(&js_sys::global(), &"dbota_replays".into())
        .ok()
        .filter(JsValue::is_object);
    match api {
        Some(api) => Box::new(Browser { api }),
        None => Box::new(Missing),
    }
}

struct Missing;

impl Backend for Missing {
    fn run(&self, _op: Op, reply: Sender<Done>) {
        let _ = reply.send(Done::Unavailable(
            "the page has no replay storage".to_owned(),
        ));
    }
}

impl Browser {
    /// Calls the page's function `name` with `args`, as a future of its result.
    fn call(&self, name: &str, args: &[JsValue]) -> Result<JsFuture, String> {
        let function: Function = Reflect::get(&self.api, &name.into())
            .ok()
            .and_then(|f| f.dyn_into().ok())
            .ok_or_else(|| format!("the page has no {name}"))?;
        let promise = function
            .apply(&self.api, &args.iter().collect::<Array>())
            .map_err(describe)?;
        Ok(JsFuture::from(
            promise.dyn_into::<Promise>().map_err(describe)?,
        ))
    }

    fn kind(&self) -> String {
        Reflect::get(&self.api, &"kind".into())
            .ok()
            .and_then(|k| k.as_string())
            .unwrap_or_default()
    }
}

fn describe(error: JsValue) -> String {
    let message = Reflect::get(&error, &"message".into())
        .ok()
        .and_then(|m| m.as_string());
    message.unwrap_or_else(|| format!("{error:?}"))
}

fn bytes_of(value: &JsValue) -> Option<Vec<u8>> {
    value.dyn_ref::<Uint8Array>().map(Uint8Array::to_vec)
}

impl Backend for Browser {
    fn run(&self, op: Op, reply: Sender<Done>) {
        let started = match &op {
            Op::Load => self.call("load", &[]),
            Op::Put { record, bytes } => self.call(
                "put",
                &[
                    (&record.name).into(),
                    Uint8Array::from(bytes.as_slice()).into(),
                ],
            ),
            Op::Get { name } => self.call("get", &[name.into()]),
            Op::Remove { names } => self.call(
                "remove",
                &[names.iter().map(JsValue::from).collect::<Array>().into()],
            ),
            Op::Index { text } => self.call("index", &[text.into()]),
            Op::Export { name } => self.call("download", &[name.into()]),
            Op::PickFile => self.call("pick", &[]),
        };
        let kind = self.kind();
        spawn_local(async move {
            let done = match started {
                Ok(future) => finish(op, future.await, &kind),
                Err(why) => failed(op, why),
            };
            let _ = reply.send(done);
        });
    }
}

/// What a finished call means for the library.
fn finish(op: Op, result: Result<JsValue, JsValue>, kind: &str) -> Done {
    let value = match result {
        Ok(value) => value,
        Err(error) => return failed(op, describe(error)),
    };
    match op {
        Op::Load => {
            let index = Reflect::get(&value, &"index".into())
                .ok()
                .and_then(|i| i.as_string());
            let files = Reflect::get(&value, &"files".into())
                .map(|f| Array::from(&f))
                .unwrap_or_default()
                .iter()
                .filter_map(|file| {
                    let field = |name: &str| Reflect::get(&file, &name.into()).ok();
                    Some((
                        field("name")?.as_string()?,
                        field("size")?.as_f64()? as u64,
                        field("modified")?.as_f64()? as u64,
                    ))
                })
                .filter(|(name, ..)| super::index::valid_name(name))
                .take(10_000)
                .collect();
            bevy::log::info!("replays: kept in the browser's {kind} storage");
            Done::Loaded { index, files }
        }
        Op::Put { record, bytes } => Done::Put {
            record: Record {
                bytes: bytes.len() as u64,
                ..record
            },
            error: None,
        },
        Op::Get { name } => Done::Got {
            result: bytes_of(&value)
                .filter(|b| b.len() as u64 <= MAX_BYTES)
                .ok_or_else(|| "that replay is not kept any more".to_owned()),
            name,
        },
        Op::PickFile => match Reflect::get(&value, &"bytes".into())
            .ok()
            .as_ref()
            .and_then(bytes_of)
        {
            Some(bytes) => Done::Picked {
                name: Reflect::get(&value, &"name".into())
                    .ok()
                    .and_then(|n| n.as_string())
                    .unwrap_or_default(),
                bytes,
            },
            // Nothing chosen.
            None => Done::PickFailed(String::new()),
        },
        Op::Remove { .. } | Op::Index { .. } | Op::Export { .. } => Done::Failed(String::new()),
    }
}

fn failed(op: Op, why: String) -> Done {
    match op {
        Op::Load => Done::Unavailable(why),
        Op::Put { record, .. } => Done::Put {
            record,
            error: Some(format!("cannot keep the replay: {why}")),
        },
        Op::Get { name } => Done::Got {
            name,
            result: Err(why),
        },
        Op::PickFile => Done::PickFailed(why),
        Op::Export { name } => Done::Failed(format!("cannot download {name}: {why}")),
        Op::Remove { .. } | Op::Index { .. } => Done::Failed(why),
    }
}
