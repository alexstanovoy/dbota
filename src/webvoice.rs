//! The page's voice worker as the client asks it (`web/voice.js`, published as
//! `globalThis.dbota_voice`): [`request`] has the worker read and check a hero's pack off the
//! page's thread and hand the pack's buffer over; [`take`] gives its records once it has, each
//! copied once into the client's memory, straight into the shared buffer its sound plays from.
//! A page without the worker answers that it has none.

use dbota_import::voice::pack::{MAX_NAME, MAX_PACK_BYTES, MAX_RECORDS};
use std::sync::Arc;

use js_sys::{Array, ArrayBuffer, Function, Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};

fn voice() -> Option<JsValue> {
    Reflect::get(&js_sys::global(), &"dbota_voice".into())
        .ok()
        .filter(JsValue::is_object)
}

fn call(name: &str, args: &Array) -> Result<JsValue, String> {
    let voice = voice().ok_or("the page has no voice worker")?;
    let function: Function = Reflect::get(&voice, &name.into())
        .ok()
        .and_then(|f| f.dyn_into().ok())
        .ok_or_else(|| format!("the page's voice worker has no {name}"))?;
    function
        .apply(&voice, args)
        .map_err(|e| format!("{name}: {e:?}"))
}

/// The answer kept for a request the page could not take.
static REFUSED: std::sync::Mutex<Option<(String, String)>> = std::sync::Mutex::new(None);

/// Asks the page's worker for `hero`'s pack at `rel` in the cache (`url` where a served cache
/// has it).
pub fn request(hero: &str, rel: &str, url: &str) {
    let args = Array::of3(&hero.into(), &rel.into(), &url.into());
    if let Err(why) = call("request", &args) {
        *REFUSED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((hero.to_owned(), why));
    }
}

/// The records of `hero`'s pack once the worker has read it, or why it could not.
pub fn take(hero: &str) -> Option<Result<Vec<(String, Arc<[u8]>)>, String>> {
    let mut refused = REFUSED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((_, why)) = refused.take_if(|(who, _)| who == hero) {
        return Some(Err(why));
    }
    drop(refused);
    let answer = match call("take", &Array::of1(&hero.into())) {
        Ok(answer) if answer.is_null() || answer.is_undefined() => return None,
        Ok(answer) => answer,
        Err(why) => return Some(Err(why)),
    };
    Some(records(&answer))
}

/// A part of the pack's buffer, copied once into a buffer of its own.
fn copied(buffer: &ArrayBuffer, start: u32, end: u32) -> Arc<[u8]> {
    let mut out: Arc<[u8]> = std::iter::repeat_n(0u8, (end - start) as usize).collect();
    let view = Uint8Array::new_with_byte_offset_and_length(buffer, start, end - start);
    view.copy_to(Arc::get_mut(&mut out).expect("a buffer of its own"));
    out
}

/// The records of a worker's answer, bounded as a pack is: each `[name, start, end]` a part of
/// the pack's buffer.
fn records(answer: &JsValue) -> Result<Vec<(String, Arc<[u8]>)>, String> {
    if let Some(error) = Reflect::get(answer, &"error".into())
        .ok()
        .and_then(|e| e.as_string())
    {
        return Err(error);
    }
    let buffer: ArrayBuffer = Reflect::get(answer, &"buffer".into())
        .ok()
        .and_then(|b| b.dyn_into().ok())
        .ok_or("the voice worker's answer has no pack")?;
    let size = buffer.byte_length();
    if size as usize > MAX_PACK_BYTES {
        return Err(format!(
            "{size} bytes, more than the {MAX_PACK_BYTES} of a pack"
        ));
    }
    let list: Array = Reflect::get(answer, &"records".into())
        .ok()
        .and_then(|r| r.dyn_into().ok())
        .ok_or("the voice worker's answer has no records")?;
    if list.length() as usize > MAX_RECORDS {
        return Err(format!("{} records, at most {MAX_RECORDS}", list.length()));
    }
    let mut out = Vec::with_capacity(list.length() as usize);
    for record in list.iter() {
        let record: Array = record
            .dyn_into()
            .map_err(|_| "a record that is not a triple")?;
        let name = record
            .get(0)
            .as_string()
            .filter(|name| name.len() <= MAX_NAME)
            .ok_or("a record without a name")?;
        let place = |at: u32| {
            record
                .get(at)
                .as_f64()
                .filter(|n| n.fract() == 0.0 && *n >= 0.0)
        };
        let (start, end) = match (place(1), place(2)) {
            (Some(start), Some(end)) if start <= end && end <= f64::from(size) => {
                (start as u32, end as u32)
            }
            _ => return Err(format!("record {name} lies outside the pack")),
        };
        out.push((name, copied(&buffer, start, end)));
    }
    Ok(out)
}
