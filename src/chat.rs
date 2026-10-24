//! Chat as Dota shows it: the log in the lower left, whose lines fade after a
//! while, and the line one types into, opened by the key table's "Chat to
//! allies" (Enter) and "Chat to all" (Shift+Enter).

use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    },
    prelude::*,
    window::{Ime, PrimaryWindow},
};
use bota_client_core::{App as Core, ChatLine, say_refusal};
use bota_core::{TextFault, rules};
use bota_proto::{Audience, Body, Speaker};

use crate::input::Typing;
use crate::keys::{Action, Fired};
use crate::session::Session;
use crate::ui::{self, Fonts};

/// Lines the log shows at most.
const ROWS: usize = 8;
/// Seconds a line stays whole, and then takes to fade away.
const FADE_AFTER: f32 = 8.0;
const FADE_SECS: f32 = 2.0;
/// The log's width in interface pixels.
const WIDTH: f32 = 420.0;
const SIZE: f32 = 14.0;
/// Seconds a fault of one's own line is shown.
const FAULT_SECS: f32 = 3.0;

/// The line being typed, while one is.
#[derive(Resource, Default)]
pub struct ChatInput {
    /// Whom it is for; absent while no line is open.
    to: Option<Audience>,
    line: String,
    /// What an input method is still composing.
    preedit: String,
    /// Why the last line was not sent, and the seconds left to show it.
    fault: Option<(String, f32)>,
}

/// The whole log, from the foot of the screen up.
#[derive(Component)]
struct Log;

/// One line of the log, from the top.
#[derive(Component)]
struct Row(usize);

/// A coloured piece of a row.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Piece {
    Name,
    Body,
}

/// The line being typed.
#[derive(Component)]
struct Prompt;

/// What was wrong with the last line sent.
#[derive(Component)]
struct Complaint;

pub fn plugin(app: &mut App) {
    app.init_resource::<ChatInput>()
        .add_systems(Startup, spawn)
        .add_systems(Update, (note_heard, show_log, show_input, ime));
    if app.world().resource::<crate::Args>().chat.is_some() {
        app.add_systems(Update, say_given.before(show_log));
    }
    app.init_resource::<Queued>()
        .add_systems(Update, say_queued.before(show_log));
}

/// Debug: lines `--script` types into the chat line and sends, one a frame.
#[derive(Resource, Default)]
pub struct Queued(pub std::collections::VecDeque<String>);

/// Sends a queued line as a person would: the line opened to everyone, the
/// text typed, Enter.
fn say_queued(
    mut session: NonSendMut<Session>,
    mut queued: ResMut<Queued>,
    mut input: ResMut<ChatInput>,
    manifest: Res<crate::manifest::Manifest>,
    cursor: Res<crate::camera::CursorGround>,
) {
    let core = &mut session.0;
    let Some(line) = queued.0.pop_front() else {
        return;
    };
    let names = crate::cheats::Names {
        manifest: &manifest,
        cursor: cursor.0.map(|p| (p.x, -p.z)),
    };
    if open(core, &mut input, true) {
        append(&mut input, &line);
        send(core, &mut input, &names);
    }
}

/// Debug `--chat`: says its lines but the last once the client may speak,
/// and opens the line with the last typed into it.
fn say_given(
    args: Res<crate::Args>,
    mut session: NonSendMut<Session>,
    mut typing: ResMut<Typing>,
    mut input: ResMut<ChatInput>,
    mut done: Local<bool>,
    manifest: Res<crate::manifest::Manifest>,
    cursor: Res<crate::camera::CursorGround>,
) {
    let core = &mut session.0;
    let names = crate::cheats::Names {
        manifest: &manifest,
        cursor: cursor.0.map(|p| (p.x, -p.z)),
    };
    let Some(given) = args.chat.as_deref().filter(|_| !*done && core.can_say()) else {
        return;
    };
    *done = true;
    let mut lines: Vec<&str> = given.split(';').collect();
    let last = lines.pop().unwrap_or_default();
    for line in lines {
        if open(core, &mut input, true) {
            append(&mut input, line);
            send(core, &mut input, &names);
        }
    }
    if open(core, &mut input, true) {
        append(&mut input, last);
        *typing = Typing::Chat;
    }
}

/// Opens the chat line when its binding fires ("Chat to allies", "Chat to
/// all") and types into it, with the keyboard and with an input method.
/// Escape drops the line, Enter sends it.
pub fn typing(
    mut session: NonSendMut<Session>,
    mut events: MessageReader<KeyboardInput>,
    mut composing: MessageReader<Ime>,
    fired: Res<Fired>,
    mut typing: ResMut<Typing>,
    mut input: ResMut<ChatInput>,
    manifest: Res<crate::manifest::Manifest>,
    cursor: Res<crate::camera::CursorGround>,
) {
    let core = &mut session.0;
    let names = crate::cheats::Names {
        manifest: &manifest,
        cursor: cursor.0.map(|p| (p.x, -p.z)),
    };
    // The press that opens the line is no keystroke of it.
    let was_open = *typing == Typing::Chat;
    // The bindings are quiet while the settings screen is up.
    if *typing == Typing::No {
        let all = fired.pressed(Action::ChatAll);
        if (all || fired.pressed(Action::ChatTeam)) && open(core, &mut input, all) {
            *typing = Typing::Chat;
        }
    }
    for event in events.read() {
        if was_open && event.state == ButtonState::Pressed && edit(core, &mut input, event, &names)
        {
            *typing = Typing::No;
        }
    }
    for event in composing.read() {
        if *typing != Typing::Chat {
            continue;
        }
        match event {
            Ime::Commit { value, .. } => {
                input.preedit.clear();
                append(&mut input, value);
            }
            Ime::Preedit { value, .. } => input.preedit.clone_from(value),
            _ => {}
        }
    }
}

/// Opens the line to everyone (`all`) or to one's own (allies, or a
/// spectator's fellows), if this client may say anything there.
fn open(core: &Core, input: &mut ChatInput, all: bool) -> bool {
    if !core.can_say() {
        return false;
    }
    let (plain, shifted) = core.chat_audiences();
    let Some(to) = (if all { shifted } else { Some(plain) }) else {
        return false;
    };
    *input = ChatInput {
        to: Some(to),
        fault: input.fault.take(),
        ..default()
    };
    true
}

/// Edits the open line with a key press. Returns whether the line is closed.
fn edit(
    core: &mut Core,
    input: &mut ChatInput,
    event: &KeyboardInput,
    names: &crate::cheats::Names,
) -> bool {
    match &event.logical_key {
        Key::Escape => {
            *input = ChatInput {
                fault: input.fault.take(),
                ..default()
            };
            return true;
        }
        Key::Enter => {
            send(core, input, names);
            return true;
        }
        Key::Backspace => {
            input.line.pop();
        }
        _ => {
            if let Some(text) = &event.text {
                append(input, text);
            }
        }
    }
    false
}

/// Appends typed text, without control characters and within the length the
/// server takes.
fn append(input: &mut ChatInput, text: &str) {
    for c in text.chars().filter(|c| !c.is_control()) {
        if input.line.len() + c.len_utf8() <= rules::MAX_CHAT_BYTES {
            input.line.push(c);
        }
    }
}

/// Sends the line: said to its audience, or, as Dota's chat takes a dash
/// and a cheat's name, run as that cheat and said to nobody.
fn send(core: &mut Core, input: &mut ChatInput, names: &crate::cheats::Names) {
    let Some(to) = input.to.take() else {
        return;
    };
    let line = std::mem::take(&mut input.line);
    input.preedit.clear();
    if line.trim().is_empty() {
        return;
    }
    if bota_client_core::is_chat_command(&line) {
        if let Err(why) = crate::cheats::run(core, &line, names) {
            input.fault = Some((why, FAULT_SECS));
        }
        return;
    }
    if let Err(fault) = core.say(to, &line) {
        input.fault = Some((fault_text(fault), FAULT_SECS));
    }
}

fn fault_text(fault: TextFault) -> String {
    match fault {
        TextFault::Empty | TextFault::Untrimmed => "There is nothing to send",
        TextFault::TooLong | TextFault::TooManyGraphemes => "That message is too long",
        TextFault::Forbidden(_) => "That message holds a character that cannot be sent",
        TextFault::Stacked => "That message stacks too many accents",
    }
    .to_owned()
}

fn prompt(to: Audience) -> &'static str {
    match to {
        Audience::Team => "To (Allies):",
        Audience::All => "To (All):",
        Audience::Spectators => "To (Spectators):",
    }
}

fn spawn(mut commands: Commands, fonts: Res<Fonts>) {
    let root = commands
        .spawn((ui::fill(), Pickable::IGNORE, GlobalZIndex(50)))
        .id();
    let log = commands
        .spawn((
            Log,
            Node {
                position_type: PositionType::Absolute,
                left: px(8),
                // Over the minimap and its margin.
                bottom: px(crate::minimap::SIDE + 14.0),
                width: px(WIDTH),
                flex_direction: FlexDirection::Column,
                row_gap: px(1),
                padding: UiRect::all(px(4)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            Pickable::IGNORE,
            ChildOf(root),
        ))
        .id();
    for row in 0..ROWS {
        spawn_row(&mut commands, &fonts, log, row);
    }
    commands.spawn((
        Complaint,
        ui::text("", &fonts.bold, SIZE, ui::HP_FOE),
        ui::shadow(),
        Node {
            display: Display::None,
            ..default()
        },
        ChildOf(log),
    ));
    commands.spawn((
        Prompt,
        ui::text("", &fonts.regular, SIZE, ui::INK),
        Node {
            display: Display::None,
            margin: UiRect::top(px(4)),
            padding: UiRect::axes(px(6), px(3)),
            ..default()
        },
        BackgroundColor(ui::PANEL_DEEP),
        ChildOf(log),
    ));
}

/// A row: the tag as its own text, then the speaker's name and the words as
/// spans.
fn spawn_row(commands: &mut Commands, fonts: &Fonts, log: Entity, row: usize) {
    let text = commands
        .spawn((
            Row(row),
            ui::text("", &fonts.regular, SIZE, ui::INK_DIM),
            ui::shadow(),
            Node {
                display: Display::None,
                ..default()
            },
            ChildOf(log),
        ))
        .id();
    for (piece, font) in [(Piece::Name, &fonts.bold), (Piece::Body, &fonts.regular)] {
        commands.spawn((
            piece,
            TextSpan::new(""),
            TextFont {
                font: font.clone().into(),
                font_size: FontSize::Px(SIZE),
                ..default()
            },
            TextColor(ui::INK),
            ChildOf(text),
        ));
    }
}

/// What a line shows before the speaker's name, as Dota's strings do.
fn tag(line: &ChatLine) -> String {
    let dead = if line.dead { "*DEAD*" } else { "" };
    match (&line.heard.from, line.heard.line.to) {
        (Speaker::Spectator(_), _) => "[Spectator] ".to_owned(),
        (Speaker::Seat(_), Audience::Team) => format!("{dead}[ALLIES] "),
        _ if line.dead => format!("{dead} "),
        _ => String::new(),
    }
}

fn name_color(core: &Core, line: &ChatLine) -> Color {
    match &line.heard.from {
        Speaker::Seat(slot) => core
            .view()
            .and_then(|view| view.player(*slot))
            .map_or(ui::INK, |p| crate::overlay::team_color(p.team)),
        Speaker::Spectator(_) => ui::INK_DIM,
    }
}

/// How much of a line is left: whole for a while, then fading out.
fn opacity(age: f32) -> f32 {
    (1.0 - (age - FADE_AFTER) / FADE_SECS).clamp(0.0, 1.0)
}

/// Says how many lines the client has heard, as they come: the one trace of
/// the chat's journey (decoded, unpacked, filtered) that a headless run can read.
fn note_heard(session: NonSend<Session>, mut seen: Local<usize>) {
    let heard = session.0.chat.lines().count();
    if heard != *seen {
        *seen = heard;
        info!("chat: {heard} lines heard");
    }
}

/// The log's lines, newest at the foot. While a line is being typed nothing
/// fades.
fn show_log(
    session: NonSend<Session>,
    typing: Res<Typing>,
    mut rows: Query<(&Row, &mut Text, &mut TextColor, &mut Node, &Children)>,
    mut pieces: Query<(&Piece, &mut TextSpan, &mut TextColor), Without<Row>>,
) {
    let core = &session.0;
    let reading = *typing == Typing::Chat;
    // A ping marks the map and says nothing in the log.
    let mut shown: Vec<(&ChatLine, &str)> = core
        .chat_lines()
        .filter_map(|line| match &line.heard.line.body {
            Body::Text(words) => Some((line, words.as_str())),
            Body::Ping(_) => None,
        })
        .rev()
        .take(ROWS)
        .collect();
    shown.reverse();
    let first = ROWS - shown.len();
    for (Row(row), mut text, mut color, mut node, children) in &mut rows {
        let line = row.checked_sub(first).and_then(|i| shown.get(i));
        let alpha = line.map_or(0.0, |(l, _)| if reading { 1.0 } else { opacity(l.age) });
        let display = if alpha > 0.0 {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        let Some(&(line, words)) = line.filter(|_| alpha > 0.0) else {
            continue;
        };
        text.set_if_neq(Text(tag(line)));
        color.set_if_neq(TextColor(ui::INK_DIM.with_alpha(alpha)));
        let name = format!("{}: ", core.speaker_name(&line.heard.from));
        for child in children {
            let Ok((piece, mut span, mut tint)) = pieces.get_mut(*child) else {
                continue;
            };
            let (s, c) = match piece {
                Piece::Name => (name.clone(), name_color(core, line)),
                Piece::Body => (words.to_owned(), ui::INK),
            };
            if span.0 != s {
                span.0 = s;
            }
            tint.set_if_neq(TextColor(c.with_alpha(alpha)));
        }
    }
}

/// The line being typed, and what went wrong with the last one sent.
fn show_input(
    mut session: NonSendMut<Session>,
    time: Res<Time>,
    typing: Res<Typing>,
    mut input: ResMut<ChatInput>,
    mut prompt_box: Single<(&mut Text, &mut Node), (With<Prompt>, Without<Complaint>)>,
    mut complaint: Single<(&mut Text, &mut Node), (With<Complaint>, Without<Prompt>)>,
    mut log: Single<&mut BackgroundColor, With<Log>>,
) {
    let core = &mut session.0;
    if let Some((_, left)) = &mut input.fault {
        *left -= time.delta_secs();
        if *left <= 0.0 {
            input.fault = None;
        }
    }
    let typing_now = *typing == Typing::Chat;
    let shade = if typing_now {
        Color::srgba(0.0, 0.0, 0.0, 0.45)
    } else {
        Color::NONE
    };
    log.set_if_neq(BackgroundColor(shade));
    let (text, node) = &mut *prompt_box;
    let line = match input.to.filter(|_| typing_now) {
        Some(to) => format!("{} {}{}_", prompt(to), input.line, input.preedit),
        None => String::new(),
    };
    text.set_if_neq(Text(line));
    let display = if typing_now {
        Display::Flex
    } else {
        Display::None
    };
    if node.display != display {
        node.display = display;
    }
    let said = input
        .fault
        .as_ref()
        .map(|(s, _)| s.clone())
        .or_else(|| core.chat.refused.map(|(reason, _)| say_refusal(reason)));
    let (text, node) = &mut *complaint;
    let display = if said.is_some() {
        Display::Flex
    } else {
        Display::None
    };
    if node.display != display {
        node.display = display;
    }
    if let Some(said) = said {
        text.set_if_neq(Text(said));
    }
}

/// Lets an input method compose into the line while one is open.
fn ime(
    typing: Res<Typing>,
    menu: Option<NonSend<crate::main_menu::MenuCore>>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
) {
    // The menu's own fields want an input method too while it covers the game.
    let want = *typing == Typing::Chat || menu.is_some_and(|menu| menu.wants_text());
    if window.ime_enabled != want {
        window.ime_enabled = want;
    }
}
