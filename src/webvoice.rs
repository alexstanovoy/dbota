//! The page's voice worker as the client asks it (`web/voice.js`, published as
//! `globalThis.dbota_voice`): [`request`] has the worker read a hero's pack off the page's
//! thread; [`take`] gives its records once it has, each moved into the client's memory by
//! itself. A page without the worker answers that it has none.

use dbota_import::voice::pack::{MAX_NAME, MAX_PACK_BYTES, MAX_RECORDS};
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
pub fn take(hero: &str) -> Option<Result<Vec<(String, Vec<u8>)>, String>> {
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

/// The records of a worker's answer, bounded as a pack is.
fn records(answer: &JsValue) -> Result<Vec<(String, Vec<u8>)>, String> {
    if let Some(error) = Reflect::get(answer, &"error".into())
        .ok()
        .and_then(|e| e.as_string())
    {
        return Err(error);
    }
    let list: Array = Reflect::get(answer, &"records".into())
        .ok()
        .and_then(|r| r.dyn_into().ok())
        .ok_or("the voice worker's answer has no records")?;
    if list.length() as usize > MAX_RECORDS {
        return Err(format!("{} records, at most {MAX_RECORDS}", list.length()));
    }
    let mut total = 0usize;
    let mut out = Vec::with_capacity(list.length() as usize);
    for record in list.iter() {
        let record: Array = record
            .dyn_into()
            .map_err(|_| "a record that is not a pair")?;
        let name = record
            .get(0)
            .as_string()
            .filter(|name| name.len() <= MAX_NAME)
            .ok_or("a record without a name")?;
        let buffer: ArrayBuffer = record
            .get(1)
            .dyn_into()
            .map_err(|_| format!("record {name} has no bytes"))?;
        total = total.saturating_add(buffer.byte_length() as usize);
        if total > MAX_PACK_BYTES {
            return Err(format!("more than the {MAX_PACK_BYTES} bytes of a pack"));
        }
        out.push((name, Uint8Array::new(&buffer).to_vec()));
    }
    Ok(out)
}
