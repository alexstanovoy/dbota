//! Platform file selection for the avatar editor.

use std::sync::mpsc::{self, Receiver, TryRecvError};

use bevy::prelude::*;

use super::avatar::{INPUT_BYTES, Request, Token};
use super::core::MenuCore;

type Selection = Result<Option<Vec<u8>>, String>;

#[derive(Default)]
pub(super) struct Picker(Option<(Token, Receiver<Selection>)>);

pub(super) fn drive(core: Option<NonSendMut<MenuCore>>, mut picker: Local<Picker>) {
    let Some(mut core) = core else {
        return;
    };
    if let Some((token, result)) = &picker.0 {
        let selection = match result.try_recv() {
            Ok(selection) => selection,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("The image chooser stopped.".to_owned()),
        };
        core.avatar_selected(*token, selection);
        picker.0 = None;
    }
    if let Some(Request::Pick { token }) = core.take_avatar_request() {
        match pick() {
            Ok(result) => picker.0 = Some((token, result)),
            Err(error) => core.avatar_selected(token, Err(error)),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn pick() -> Result<Receiver<Selection>, String> {
    let (send, receive) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("avatar-picker".to_owned())
        .spawn(move || {
            let _ = send.send(read_chosen_file());
        })
        .map_err(|error| format!("Could not open the image chooser: {error}"))?;
    Ok(receive)
}

#[cfg(not(target_arch = "wasm32"))]
fn read_chosen_file() -> Selection {
    use std::io::Read;

    let path = crate::desktop::ask_file("Choose a PNG or JPEG image").map_err(|error| match error {
        crate::desktop::NoDialog::Missing => "No system file chooser is available.".to_owned(),
        crate::desktop::NoDialog::Failed(message) => format!("Could not open the image chooser: {message}"),
    })?;
    let Some(path) = path else {
        return Ok(None);
    };
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let unreadable = |error: std::io::Error| format!("Could not read the chosen image: {error}");
    let file = options.open(path).map_err(unreadable)?;
    let metadata = file.metadata().map_err(unreadable)?;
    if !metadata.is_file() {
        return Err("Choose an image file.".to_owned());
    }
    let limit = INPUT_BYTES as u64;
    if metadata.len() > limit {
        return Err(too_large());
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes).map_err(unreadable)?;
    if bytes.len() > INPUT_BYTES {
        return Err(too_large());
    }
    Ok(Some(bytes))
}

fn too_large() -> String {
    format!("Choose a PNG or JPEG up to {} MB.", INPUT_BYTES >> 20)
}

#[cfg(target_arch = "wasm32")]
fn pick() -> Result<Receiver<Selection>, String> {
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = dbota_page, js_name = pickAvatar, catch)]
        fn pick_avatar(limit: u32) -> Result<js_sys::Promise, JsValue>;
    }

    let unavailable = || "Could not open the browser's image chooser.".to_owned();
    let answer = pick_avatar(INPUT_BYTES as u32).map_err(|_| unavailable())?;
    let (send, receive) = mpsc::sync_channel(1);
    wasm_bindgen_futures::spawn_local(async move {
        let result = wasm_bindgen_futures::JsFuture::from(answer)
            .await
            .map_err(|error| error.as_string().unwrap_or_else(unavailable))
            .and_then(|value| {
                if value.is_null() {
                    return Ok(None);
                }
                let bytes = value.dyn_into::<js_sys::Uint8Array>().map_err(|_| unavailable())?;
                if bytes.length() as usize > INPUT_BYTES {
                    return Err(too_large());
                }
                Ok(Some(bytes.to_vec()))
            });
        let _ = send.send(result);
    });
    Ok(receive)
}
