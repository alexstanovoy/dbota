//! Avatar editor intent/state. The SDK/platform adapter supplies the picker and save backend;
//! unavailable services keep the editor honest and cannot dispatch an upload.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "The disabled avatar editor retains its validated adapter seam until the service SDK exists"
    )
)]
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, futures_lite::future};
use dbota_import::thumbnail::{self, Pixels};
use std::sync::Arc;

pub const STORED_BYTES: usize = 512;
const INPUT_BYTES: usize = 256 * 1024;
pub const SIDE: u32 = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Open,
    Upload,
    Remove,
    Save,
    Cancel,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub account: u64,
    pub generation: u64,
}
#[derive(Clone, Debug)]
pub enum Request {
    Pick {
        token: Token,
    },
    /// A UI intent, not a wire message. The backend adapter encodes the crop into its bounded
    /// service format; only a validated compact image can be published as the saved avatar.
    Save {
        token: Token,
        crop: Option<Arc<Pixels>>,
    },
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct View {
    pub open: bool,
    pub available: bool,
    pub busy: bool,
    pub current: Option<Arc<Pixels>>,
    pub preview: Option<Arc<Pixels>>,
    pub notice: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operation {
    Pick,
    Decode,
    Save,
}
type Decoding = Task<Result<Pixels, String>>;
#[derive(Default)]
pub struct Editor {
    pub view: View,
    account: Option<u64>,
    generation: u64,
    dirty: bool,
    operation: Option<Operation>,
    request: Option<Request>,
    decoding: Option<(Token, Decoding)>,
}
impl Editor {
    pub fn backend(&mut self, account: Option<u64>, available: bool) {
        let available = available && account.is_some();
        if self.account == account && self.view.available == available {
            return;
        }
        self.generation = self.generation.wrapping_add(1);
        self.request = None;
        self.decoding = None;
        self.operation = None;
        self.view.busy = false;
        if self.account != account {
            self.view = View::default();
            self.dirty = false;
        }
        self.account = account;
        self.view.available = available;
        if !available {
            self.view.open = false;
            self.view.preview = None;
            self.view.notice = None;
            self.dirty = false;
        }
    }
    fn token(&self) -> Option<Token> {
        self.account.map(|account| Token {
            account,
            generation: self.generation,
        })
    }
    fn owns(&self, token: Token) -> bool {
        self.token() == Some(token)
    }
    pub fn act(&mut self, action: Action) {
        if action == Action::Open {
            if self.view.busy || !self.view.available {
                return;
            }
            self.generation = self.generation.wrapping_add(1);
            self.view.open = true;
            self.view.preview = self.view.current.clone();
            self.dirty = false;
            self.view.notice = None;
            return;
        }
        if action == Action::Cancel && !self.view.busy {
            self.generation = self.generation.wrapping_add(1);
            self.view.open = false;
            self.view.preview = None;
            self.request = None;
            return;
        }
        if !self.view.open || self.view.busy || !self.view.available {
            return;
        }
        if matches!(action, Action::Upload | Action::Save) {
            self.generation = self.generation.wrapping_add(1);
        }
        let Some(token) = self.token() else {
            return;
        };
        match action {
            Action::Upload => {
                self.request = Some(Request::Pick { token });
                self.operation = Some(Operation::Pick);
                self.view.notice = None;
                self.view.busy = true;
            }
            Action::Remove => {
                self.view.preview = None;
                self.dirty = true;
                self.view.notice = None;
            }
            Action::Save if !self.dirty => self.view.open = false,
            Action::Save => {
                self.request = Some(Request::Save {
                    token,
                    crop: self.view.preview.clone(),
                });
                self.operation = Some(Operation::Save);
                self.view.busy = true;
            }
            Action::Open | Action::Cancel => {}
        }
    }
    pub fn sync_owner(&mut self, account: Option<u64>) {
        if self.account != account {
            self.backend(account, false);
        }
    }
    pub fn take_request(&mut self) -> Option<Request> {
        self.request.take()
    }
    pub fn selected(&mut self, token: Token, bytes: Option<Vec<u8>>) {
        if !self.owns(token)
            || !self.view.open
            || !self.view.available
            || self.operation != Some(Operation::Pick)
        {
            return;
        }
        let Some(bytes) = bytes else {
            self.view.busy = false;
            self.operation = None;
            return;
        };
        if bytes.len() > INPUT_BYTES {
            self.view.busy = false;
            self.operation = None;
            self.view.notice = Some("Choose a PNG or JPEG up to 256 KB.".to_owned());
            return;
        }
        if self.decoding.is_some() {
            return;
        }
        self.operation = Some(Operation::Decode);
        self.view.notice = Some("Reading image…".to_owned());
        self.decoding = Some((
            token,
            AsyncComputeTaskPool::get()
                .spawn(async move { thumbnail::decode(&bytes, INPUT_BYTES, 4096)?.square(SIDE) }),
        ));
    }
    pub fn poll(&mut self) {
        let Some((token, task)) = &mut self.decoding else {
            return;
        };
        let Some(result) = block_on(future::poll_once(task)) else {
            return;
        };
        let token = *token;
        self.decoding = None;
        if !self.owns(token) {
            return;
        }
        self.view.busy = false;
        self.operation = None;
        match result {
            Ok(pixels) => {
                self.view.preview = Some(Arc::new(pixels));
                self.view.notice = None;
                self.dirty = true;
            }
            Err(why) => self.view.notice = Some(why),
        }
    }
    pub fn saved(&mut self, token: Token, result: Result<Option<Vec<u8>>, String>) {
        if !self.owns(token) || self.operation != Some(Operation::Save) {
            return;
        }
        self.view.busy = false;
        self.operation = None;
        let pixels =
            result.and_then(|bytes| bytes.map(|bytes| stored(&bytes).map(Arc::new)).transpose());
        match pixels {
            Ok(pixels) => {
                self.view.current = pixels;
                self.view.open = false;
                self.view.preview = None;
                self.dirty = false;
                self.view.notice = None;
            }
            Err(why) => self.view.notice = Some(why),
        }
    }
}

pub fn stored(bytes: &[u8]) -> Result<Pixels, String> {
    let pixels = thumbnail::decode(bytes, STORED_BYTES, SIDE)?;
    if pixels.width() != pixels.height() {
        return Err("The service supplied a non-square avatar.".to_owned());
    }
    pixels.square(SIDE)
}

impl super::core::MenuCore {
    pub fn avatar_backend(&mut self, available: bool) {
        let account = self
            .client
            .as_ref()
            .and_then(|client| client.me())
            .map(|me| me.id.0);
        self.avatar.backend(account, available);
    }
    pub fn take_avatar_request(&mut self) -> Option<Request> {
        self.avatar.take_request()
    }
    pub fn avatar_selected(&mut self, token: Token, bytes: Option<Vec<u8>>) {
        self.avatar.selected(token, bytes);
    }
    pub fn avatar_saved(&mut self, token: Token, result: Result<Option<Vec<u8>>, String>) {
        self.avatar.saved(token, result);
    }
    pub(super) fn update_avatar(&mut self) {
        let account = self
            .account
            .then(|| {
                self.client
                    .as_ref()
                    .and_then(|client| client.me())
                    .map(|me| me.id.0)
                    .or_else(|| self.account_profile.as_ref().map(|profile| profile.id.0))
            })
            .flatten();
        self.avatar.sync_owner(account);
        self.avatar.poll();
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) const TEST_PNG: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6,
        0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 16, 137, 218, 242,
        31, 0, 3, 202, 2, 34, 16, 220, 135, 63, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];
    #[test]
    fn a_missing_backend_never_issues_an_upload_or_claims_a_save() {
        let mut editor = Editor::default();
        editor.backend(Some(1), false);
        editor.act(Action::Open);
        editor.act(Action::Upload);
        editor.act(Action::Save);
        assert!(editor.take_request().is_none());
        assert!(!editor.view.open);
        assert!(editor.view.notice.is_none());
    }
    #[test]
    fn an_earlier_accounts_picker_result_cannot_enter_a_new_accounts_editor() {
        let mut editor = Editor::default();
        editor.backend(Some(1), true);
        editor.act(Action::Open);
        editor.act(Action::Upload);
        let Some(Request::Pick { token }) = editor.take_request() else {
            panic!("picker intent");
        };
        editor.backend(Some(2), true);
        editor.act(Action::Open);
        editor.selected(token, Some(vec![0; 20]));
        assert!(editor.decoding.is_none());
        assert!(editor.view.preview.is_none());
        assert!(!editor.view.busy);
    }
    #[test]
    fn compact_service_images_never_exceed_the_declared_byte_budget() {
        assert!(
            stored(&vec![0; STORED_BYTES + 1])
                .unwrap_err()
                .contains("512")
        );
    }
}
