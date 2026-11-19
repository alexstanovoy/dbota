//! Avatar editing, from a picked file to the image committed by the service.
use super::avatar_cache::Wish;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on};
use bota_radovid::proto::{AvatarVersion, Error, Reply};
use dbota_import::thumbnail::{self, Pixels};
use std::sync::Arc;

/// The side of the picture an avatar is made at: what the service stores.
pub const SIDE: u32 = super::avatar_image::SIDE;
/// The most bytes of a picked file; a phone's photograph is a few megabytes.
pub(super) const INPUT_BYTES: usize = thumbnail::MAX_INPUT_BYTES;
/// The most pixels along either side of a picked picture.
const INPUT_SIDE: u32 = thumbnail::MAX_SIDE;

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
/// What the platform adapter is asked for: it alone can open a file chooser.
#[derive(Clone, Debug)]
pub enum Request {
    Pick { token: Token },
}
/// The finished avatar to publish, or `None` to remove the avatar: the service's format, made
/// by the editor, so that the sender only has to send it.
#[derive(Clone, Debug)]
pub struct Upload {
    pub token: Token,
    pub png: Option<Vec<u8>>,
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
    Encode,
    Save,
}
type Decoding = Task<Result<Pixels, String>>;
type Encoding = Task<Result<Vec<u8>, String>>;
#[derive(Default)]
pub struct Editor {
    pub view: View,
    account: Option<u64>,
    generation: u64,
    dirty: bool,
    operation: Option<Operation>,
    request: Option<Request>,
    upload: Option<Upload>,
    /// The version of `view.current`, and the decoding of a newer one the service has.
    current_version: Option<AvatarVersion>,
    loading: Option<(AvatarVersion, Decoding)>,
    decoding: Option<(Token, Decoding)>,
    encoding: Option<(Token, Encoding)>,
}
impl Editor {
    pub fn backend(&mut self, account: Option<u64>, available: bool) {
        let available = available && account.is_some();
        if self.account == account && self.view.available == available {
            return;
        }
        self.generation = self.generation.wrapping_add(1);
        self.request = None;
        self.upload = None;
        self.decoding = None;
        self.encoding = None;
        self.operation = None;
        self.view.busy = false;
        if self.account != account {
            self.view = View::default();
            self.current_version = None;
            self.loading = None;
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
                self.view.busy = true;
                self.view.notice = None;
                match self.view.preview.clone() {
                    Some(picture) => {
                        self.operation = Some(Operation::Encode);
                        self.view.notice = Some("Preparing the avatar…".to_owned());
                        self.encoding = Some((
                            token,
                            AsyncComputeTaskPool::get().spawn(async move { encode(&picture) }),
                        ));
                    }
                    None => {
                        self.operation = Some(Operation::Save);
                        self.upload = Some(Upload { token, png: None });
                    }
                }
            }
            Action::Open | Action::Cancel => {}
        }
    }
    pub fn take_request(&mut self) -> Option<Request> {
        self.request.take()
    }
    pub fn take_upload(&mut self) -> Option<Upload> {
        self.upload.take()
    }
    pub fn selected(&mut self, token: Token, file: Result<Option<Vec<u8>>, String>) {
        if !self.owns(token)
            || !self.view.open
            || !self.view.available
            || self.operation != Some(Operation::Pick)
        {
            return;
        }
        let bytes = match file {
            Ok(Some(bytes)) => bytes,
            cancelled => {
                self.view.busy = false;
                self.operation = None;
                self.view.notice = cancelled.err();
                return;
            }
        };
        if bytes.len() > INPUT_BYTES {
            self.view.busy = false;
            self.operation = None;
            self.view.notice = Some(format!(
                "Choose a PNG or JPEG up to {} MB.",
                INPUT_BYTES >> 20
            ));
            return;
        }
        if self.decoding.is_some() {
            return;
        }
        self.operation = Some(Operation::Decode);
        self.view.notice = Some("Reading image…".to_owned());
        self.decoding = Some((
            token,
            AsyncComputeTaskPool::get().spawn(async move {
                thumbnail::decode(&bytes, INPUT_BYTES, INPUT_SIDE)?.square(SIDE)
            }),
        ));
    }
    pub fn poll(&mut self) {
        self.poll_with(false);
    }
    /// Waits for the work in the background to finish and takes it in, for tests, which cannot
    /// watch a clock.
    #[cfg(test)]
    pub fn finish(&mut self) {
        self.poll_with(true);
    }
    fn poll_with(&mut self, wait: bool) {
        self.poll_decoding(wait);
        self.poll_encoding(wait);
        self.poll_loading(wait);
    }
    fn poll_decoding(&mut self, wait: bool) {
        let Some((token, task)) = &mut self.decoding else {
            return;
        };
        let Some(result) = ready(task, wait) else {
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
    fn poll_encoding(&mut self, wait: bool) {
        let Some((token, task)) = &mut self.encoding else {
            return;
        };
        let Some(result) = ready(task, wait) else {
            return;
        };
        let token = *token;
        self.encoding = None;
        if !self.owns(token) {
            return;
        }
        match result {
            Ok(png) => {
                self.operation = Some(Operation::Save);
                self.view.notice = None;
                self.upload = Some(Upload {
                    token,
                    png: Some(png),
                });
            }
            Err(why) => {
                self.view.busy = false;
                self.operation = None;
                self.view.notice = Some(why);
            }
        }
    }
    /// The service answered an upload: the avatar it stored (its version and bytes), none after a
    /// removal, or why it refused.
    pub fn saved(
        &mut self,
        token: Token,
        result: Result<Option<(AvatarVersion, Vec<u8>)>, String>,
    ) {
        if !self.owns(token) || self.operation != Some(Operation::Save) {
            return;
        }
        self.view.busy = false;
        self.operation = None;
        let stored = result.and_then(|saved| {
            saved
                .map(|(version, bytes)| stored(&bytes).map(|pixels| (version, Arc::new(pixels))))
                .transpose()
        });
        match stored {
            Ok(saved) => {
                self.current_version = saved.as_ref().map(|(version, _)| *version);
                self.view.current = saved.map(|(_, pixels)| pixels);
                self.loading = None;
                self.view.open = false;
                self.view.preview = None;
                self.dirty = false;
                self.view.notice = None;
            }
            Err(why) => self.view.notice = Some(why),
        }
    }

    /// The version of the avatar the account's profile says it has: fetched, unless the picture
    /// shown already is that one.
    pub fn wants(&self, profile: Option<AvatarVersion>) -> Option<AvatarVersion> {
        let busy = self.loading.is_some() || self.operation.is_some();
        profile.filter(|version| !busy && self.current_version != Some(*version))
    }

    /// The profile says the account has no avatar (any more): the picture shown goes.
    pub fn without_avatar(&mut self) {
        if self.operation.is_none() && self.current_version.is_some() {
            self.view.current = None;
            self.current_version = None;
            self.loading = None;
        }
    }

    /// The bytes of the avatar of `version` arrived; they are decoded off the frame loop.
    pub fn load(&mut self, version: AvatarVersion, bytes: Vec<u8>) {
        self.loading = Some((
            version,
            AsyncComputeTaskPool::get().spawn(async move { stored(&bytes) }),
        ));
    }

    /// No avatar is to be had for the profile's version: it is not asked for again.
    pub fn unavailable(&mut self, version: AvatarVersion) {
        self.current_version = Some(version);
    }

    fn poll_loading(&mut self, wait: bool) {
        let Some((version, task)) = &mut self.loading else {
            return;
        };
        let Some(result) = ready(task, wait) else {
            return;
        };
        let version = *version;
        self.loading = None;
        self.current_version = Some(version);
        match result {
            Ok(pixels) if self.operation.is_none() => self.view.current = Some(Arc::new(pixels)),
            Ok(_) | Err(_) => {}
        }
    }
}

/// The task's result if it is done (or, when waiting, once it is).
fn ready<T>(task: &mut Task<T>, wait: bool) -> Option<T> {
    if wait {
        Some(block_on(task))
    } else {
        crate::pacing::poll(task)
    }
}

/// The picture of an avatar the service sent.
pub fn stored(bytes: &[u8]) -> Result<Pixels, String> {
    super::avatar_image::decode(bytes)
}

/// The avatar of a picture of the avatar size: its colours reduced only as far as the service's
/// limit needs.
pub fn encode(picture: &Pixels) -> Result<Vec<u8>, String> {
    if (picture.width(), picture.height()) != (SIDE, SIDE) {
        return Err("The picture is not the avatar size.".to_owned());
    }
    radovid_proto::encode_avatar_rgba(picture.rgba()).map_err(|why| why.to_string())
}

impl super::core::MenuCore {
    /// The platform's part of the editor: a picture to choose. Everything after the choice is the
    /// menu's own.
    pub fn take_avatar_request(&mut self) -> Option<Request> {
        self.avatar.take_request()
    }
    pub fn avatar_selected(&mut self, token: Token, file: Result<Option<Vec<u8>>, String>) {
        self.avatar.selected(token, file);
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
        self.avatar.backend(account, self.account);
        self.avatar.poll();
        self.send_avatar_upload();
        self.fetch_own_avatar();
    }

    /// Sends the avatar the editor finished (or the removal of it) to the service.
    fn send_avatar_upload(&mut self) {
        let Some(Upload { token, png }) = self.avatar.take_upload() else {
            return;
        };
        let sent = match (&mut self.client, png) {
            (Some(client), Some(png)) => client.set_avatar(&png),
            (Some(client), None) => client.remove_avatar(),
            (None, _) => Err(bota_radovid::ClientError::Closed),
        };
        match sent {
            Ok(sequence) => self.track_request(sequence, super::core::Asked::AvatarSaved(token)),
            Err(bota_radovid::ClientError::Refused(error)) => {
                self.avatar.saved(token, Err(refusal_text(&error)))
            }
            Err(_) => self.avatar.saved(
                token,
                Err("The avatar could not be sent: the connection is down.".to_owned()),
            ),
        }
    }

    pub(super) fn avatar_saved_reply(&mut self, token: Token, reply: Reply) {
        let result = match reply {
            Reply::Avatar { avatar, .. } => {
                let version = avatar.as_ref().map(|avatar| avatar.version);
                // The profile and the lists name the avatar by version: the new one is named now.
                let own = self.my_id();
                if let (Some(profile), Some(own)) = (self.account_profile.as_mut(), own) {
                    profile.avatar = version;
                    self.avatars.forget(own.0);
                }
                Ok(avatar.map(|avatar| (avatar.version, avatar.png)))
            }
            _ => Err("The service returned an invalid answer.".to_owned()),
        };
        self.avatar.saved(token, result);
    }

    /// The account's own avatar, small, for the card that names the player.
    pub(super) fn own_list_avatar(&self) -> Option<Arc<Pixels>> {
        let own = self.my_id()?.0;
        let hint = self.account_profile.as_ref()?.avatar;
        self.avatars.picture(own, hint)
    }

    /// Takes in the pictures that arrived and asks for the ones the rows drawn name and the menu
    /// does not hold.
    pub(super) fn drive_avatars(&mut self) {
        self.avatars.poll();
        if !self.link_ready() {
            return;
        }
        let own = self.my_id().map(|id| id.0).zip(
            self.account_profile
                .as_ref()
                .and_then(|profile| profile.avatar),
        );
        let rows = (self.page == super::core::Page::Leaderboard)
            .then(|| self.board.rows.iter().chain(self.board.own.iter()))
            .into_iter()
            .flatten()
            .map(|row| Wish {
                account: row.account,
                version: row.avatar,
            });
        let wishes = own
            .map(|(account, version)| Wish {
                account,
                version: Some(version),
            })
            .into_iter()
            .chain(rows);
        let now = bevy::platform::time::Instant::now();
        let Some(queries) = self.avatars.next_ask(wishes, now) else {
            return;
        };
        let Some(client) = self.client.as_mut() else {
            return;
        };
        match client.get_avatars(&queries) {
            Ok(sequence) => self.track_request(sequence, super::core::Asked::Avatars),
            Err(bota_radovid::ClientError::Refused(error)) => self.avatars.refused(&error, now),
            Err(_) => self.avatars.refused(&Error::Internal, now),
        }
    }

    pub(super) fn avatar_saved_refused(&mut self, token: Token, error: &Error) {
        self.avatar.saved(token, Err(refusal_text(error)));
    }

    /// Asks for the account's own avatar when its profile names one that is not shown yet.
    fn fetch_own_avatar(&mut self) {
        let Some(own) = self.my_id() else {
            return;
        };
        let Some(profile) = self.account_profile.as_ref() else {
            return;
        };
        let hint = profile.avatar;
        if hint.is_none() {
            self.avatar.without_avatar();
        }
        let Some(version) = self.avatar.wants(hint) else {
            return;
        };
        if self.own_avatar_asked == Some(version) {
            return;
        }
        let Some(client) = self.client.as_mut() else {
            return;
        };
        match client.get_avatar(own) {
            Ok(sequence) => {
                self.own_avatar_asked = Some(version);
                self.track_request(sequence, super::core::Asked::OwnAvatar(version));
            }
            Err(_) => self.own_avatar_asked = Some(version),
        }
    }

    pub(super) fn own_avatar_reply(&mut self, version: AvatarVersion, reply: Reply) {
        self.own_avatar_asked = None;
        match reply {
            Reply::Avatar {
                avatar: Some(avatar),
                ..
            } => self.avatar.load(avatar.version, avatar.png),
            _ => self.avatar.unavailable(version),
        }
    }

    pub(super) fn own_avatar_refused(&mut self, version: AvatarVersion) {
        self.avatar.unavailable(version);
    }
}

/// What a person is told when the service did not take their avatar.
fn refusal_text(error: &Error) -> String {
    match error {
        Error::Throttled { retry_after_ms } => format!(
            "The avatar can be changed once a minute. Try again in {} s.",
            retry_after_ms.div_ceil(1000)
        ),
        Error::BadAvatar => "The service did not accept this picture.".to_owned(),
        Error::UnknownAccount | Error::NotLoggedIn | Error::BadSession => {
            "Sign in again to change the avatar.".to_owned()
        }
        _ => "The avatar could not be saved right now.".to_owned(),
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
        editor.selected(token, Ok(Some(vec![0; 20])));
        assert!(editor.decoding.is_none());
        assert!(editor.view.preview.is_none());
        assert!(!editor.view.busy);
    }
    #[test]
    fn service_images_beyond_the_byte_limit_are_refused_with_the_formats_reason() {
        assert_eq!(
            stored(&vec![0; radovid_proto::MAX_AVATAR_BYTES + 1]).unwrap_err(),
            "avatar refused: more bytes than the avatar limit"
        );
    }

    fn pool() {
        bevy::tasks::AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    }

    /// What the editor makes of a picked file: the bytes it would upload, or what it tells the
    /// person.
    fn edited(file: Vec<u8>) -> Result<Vec<u8>, String> {
        pool();
        let mut editor = Editor::default();
        editor.backend(Some(1), true);
        editor.act(Action::Open);
        editor.act(Action::Upload);
        let Some(Request::Pick { token }) = editor.take_request() else {
            panic!("the editor asks for a picture");
        };
        editor.selected(token, Ok(Some(file)));
        editor.finish();
        if editor.view.preview.is_none() {
            return Err(editor.view.notice.expect("a refusal is explained"));
        }
        editor.act(Action::Save);
        editor.finish();
        let upload = editor.take_upload().expect("a finished upload");
        assert_eq!(upload.token, editor.token().expect("token"));
        Ok(upload.png.expect("a picture, not a removal"))
    }

    fn png(
        width: u32,
        height: u32,
        color: png::ColorType,
        depth: png::BitDepth,
        data: &[u8],
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, width, height);
            encoder.set_color(color);
            encoder.set_depth(depth);
            encoder
                .write_header()
                .expect("header")
                .write_image_data(data)
                .expect("pixels");
        }
        bytes
    }

    /// Smooth colour with grain, like a photograph, `side` pixels square.
    fn photograph(side: u32) -> Vec<u8> {
        let mut state = 5u64;
        let data: Vec<u8> = (0..side * side)
            .flat_map(|index| {
                let (y, x) = ((index / side) as f32, (index % side) as f32);
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let grain = (state >> 59) as f32;
                [
                    (128.0 + 100.0 * (x / 40.0).sin() + grain) as u8,
                    (128.0 + 100.0 * (y / 55.0).cos() + grain) as u8,
                    (60.0 + x * 190.0 / side as f32) as u8,
                ]
            })
            .collect();
        png(side, side, png::ColorType::Rgb, png::BitDepth::Eight, &data)
    }

    #[test]
    fn what_the_editor_makes_of_any_picked_picture_passes_the_services_check() {
        let flat = |w: u32, h: u32, px: &[u8]| {
            png(
                w,
                h,
                png::ColorType::Rgba,
                png::BitDepth::Eight,
                &px.repeat((w * h) as usize),
            )
        };
        let animated = {
            let mut bytes = Vec::new();
            {
                let mut encoder = png::Encoder::new(&mut bytes, 4, 4);
                encoder.set_color(png::ColorType::Rgb);
                encoder.set_depth(png::BitDepth::Eight);
                encoder.set_animated(2, 0).expect("two frames");
                let mut writer = encoder.write_header().expect("header");
                writer
                    .write_image_data(&[200; 4 * 4 * 3])
                    .expect("first frame");
                writer
                    .write_image_data(&[10; 4 * 4 * 3])
                    .expect("second frame");
            }
            bytes
        };
        let sixteen_bit: Vec<u8> = (0..64 * 64 * 4 * 2).map(|i| (i % 251) as u8).collect();
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("a huge picture", flat(4096, 4096, &[30, 120, 200, 255])),
            ("a one pixel wide strip", flat(1, 3000, &[200, 40, 40, 255])),
            ("a one pixel tall strip", flat(3000, 1, &[40, 200, 40, 255])),
            ("a one pixel picture", flat(1, 1, &[1, 2, 3, 255])),
            ("a fully transparent picture", flat(300, 200, &[9, 9, 9, 0])),
            (
                "a half transparent picture",
                flat(100, 100, &[250, 250, 0, 128]),
            ),
            (
                "sixteen bits per sample",
                png(
                    64,
                    64,
                    png::ColorType::Rgba,
                    png::BitDepth::Sixteen,
                    &sixteen_bit,
                ),
            ),
            (
                "grayscale with alpha",
                png(
                    8,
                    8,
                    png::ColorType::GrayscaleAlpha,
                    png::BitDepth::Eight,
                    &[77; 8 * 8 * 2],
                ),
            ),
            ("an animated picture", animated),
            ("a photograph", photograph(700)),
            ("a photograph smaller than the avatar", photograph(90)),
        ];
        for (what, file) in cases {
            let avatar = edited(file).unwrap_or_else(|why| panic!("{what}: {why}"));
            assert!(
                avatar.len() <= radovid_proto::MAX_AVATAR_BYTES,
                "{what}: {} bytes",
                avatar.len()
            );
            assert_eq!(radovid_proto::validate_avatar(&avatar), Ok(()), "{what}");
            assert_eq!(
                radovid_proto::canonical_avatar(&avatar).expect("canonical"),
                avatar,
                "{what}: the service would store exactly these bytes"
            );
        }
    }

    #[test]
    fn a_picked_file_that_is_no_picture_is_explained_and_never_becomes_an_upload() {
        let truncated = {
            let mut file = photograph(64);
            file.truncate(file.len() / 2);
            file
        };
        let decode_failed = "The image could not be decoded; choose a PNG or JPEG image.";
        for (file, explained) in [
            (b"not an image".to_vec(), "Choose a PNG or JPEG image."),
            (vec![0xff, 0xd8, 0xff, 0xe0, 0, 0], decode_failed),
            (truncated, decode_failed),
        ] {
            assert_eq!(edited(file).unwrap_err(), explained);
        }
    }

    #[test]
    fn a_file_beyond_the_size_limit_is_refused_before_any_decoding() {
        assert_eq!(
            edited(vec![0; INPUT_BYTES + 1]).unwrap_err(),
            "Choose a PNG or JPEG up to 16 MB."
        );
    }

    #[test]
    fn removing_the_avatar_uploads_a_removal_without_encoding() {
        pool();
        let mut editor = Editor::default();
        editor.backend(Some(1), true);
        editor.act(Action::Open);
        editor.act(Action::Remove);
        editor.act(Action::Save);
        let upload = editor.take_upload().expect("a removal");
        assert_eq!(upload.png, None);
    }
}
