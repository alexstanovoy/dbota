//! The lobby boards, using the approved example conversation and roster.
use super::*;
use crate::main_menu::{
    lobby_chat::{ChatView, Line, Person},
    lobby_controls::{Choice, ControlsView, EditorView},
    view::{LobbyRow, LobbyView, ModeRow, SettingsView, SlotRow, TeamRow},
};
use bota_radovid::proto::Audience;

fn modes() -> Vec<ModeRow> {
    [("full", "All Pick"), ("solo_mid", "Solo Mid")]
        .into_iter()
        .map(|(id, name)| ModeRow {
            id: RulesetId(id.into()),
            name: name.into(),
            chosen: id == "full",
        })
        .collect()
}

fn browser_rows() -> Vec<LobbyRow> {
    [
        (
            "Friday stack",
            "kestrel#118",
            "full",
            "All Pick",
            true,
            7,
            10,
        ),
        (
            "mid duels, any rating",
            "tamsin#3311",
            "solo_mid",
            "Solo Mid",
            false,
            1,
            2,
        ),
        (
            "bots practice",
            "lowtide#7",
            "full",
            "All Pick",
            false,
            2,
            10,
        ),
        ("EU evening", "owlbear#90", "full", "All Pick", true, 9, 10),
        (
            "learn the hook",
            "hookline#4",
            "solo_mid",
            "Solo Mid",
            false,
            1,
            2,
        ),
        (
            "sandbox, cheats on",
            "quartz#55",
            "full",
            "All Pick",
            false,
            1,
            10,
        ),
    ]
    .into_iter()
    .enumerate()
    .map(
        |(index, (name, host, id, mode, locked, members, seats))| LobbyRow {
            id: LobbyId(index as u32 + 1),
            name: name.into(),
            owner: host.into(),
            ruleset: RulesetId(id.into()),
            mode: mode.into(),
            locked,
            members,
            seats,
        },
    )
    .collect()
}

pub(super) fn browser(password: bool) -> MenuView {
    let list = browser_rows();
    let mut view = logged_in(
        Page::Lobbies,
        PageView::Lobbies {
            list,
            fields: vec![field("Lobby or host name", "", false)],
        },
    );
    view.player = "mira".into();
    view.account = Some(view.player.clone());
    view.lobby_ui = ControlsView {
        modes: modes(),
        mode: Some(RulesetId("full".into())),
        selected: Some(LobbyId(1)),
        open: (!password).then_some(Choice::Server),
        ..Default::default()
    };
    if password {
        view.password = Some(PasswordView {
            lobby: "Friday stack".into(),
            field: field("Password", "••••••", false),
            error: Some("Wrong password.".into()),
        });
    }
    view
}

fn member(account: u64, name: &str, ready: bool, me: bool) -> MemberRow {
    MemberRow {
        bot: false,
        account: AccountId(account),
        name: name.into(),
        ready,
        me,
    }
}

fn team(name: &str, first: u8, members: Vec<MemberRow>) -> TeamRow {
    let mut members = members.into_iter();
    TeamRow {
        name: name.into(),
        slots: (first..first + 5)
            .map(|slot| SlotRow {
                slot,
                who: members.next(),
            })
            .collect(),
    }
}

fn roster(start: bool) -> (Vec<MemberRow>, Vec<MemberRow>) {
    let radiant = vec![
        member(118, "kestrel#118", start, true),
        member(2207, "mira#2207", start, false),
        member(7, "lowtide#7", true, false),
    ];
    let dire = vec![
        member(3311, "tamsin#3311", true, false),
        member(90, "owlbear#90", start, false),
    ];
    let (mut radiant, mut dire) = (radiant, dire);
    if start {
        radiant.extend([
            member(201, "vale#201", true, false),
            member(62, "brine#62", true, false),
        ]);
        dire.extend([
            member(9, "sorrel#9", true, false),
            member(771, "nim#771", true, false),
            member(40, "juniper#40", true, false),
        ]);
    }
    (radiant, dire)
}

pub(super) fn room(name: &str) -> MenuView {
    let start = name == "lobby-start";
    let (radiant, dire) = roster(start);
    let owner = name != "lobby";
    let lobby = LobbyView {
        name: "Friday stack".into(),
        locked: true,
        teams: vec![team("RADIANT", 0, radiant), team("DIRE", 5, dire)],
        spectators: vec![
            member(55, "quartz#55", false, false),
            member(4, "hookline#4", false, false),
        ],
        owner,
        moving: None,
        settings: SettingsView {
            mode: "All Pick".into(),
            audience: "Humans and bots",
            locked: true,
            modes: if owner { modes() } else { Vec::new() },
            audiences: Vec::new(),
            cheats: Some(false),
            editable: owner,
        },
        me_ready: start,
        waiting: if start {
            None
        } else {
            Some(crate::main_menu::view::LobbyBlock::Ready(2))
        },
        starts_short: false,
        starting_in_secs: None,
    };
    let mut lobby = lobby;
    for member in lobby
        .teams
        .iter_mut()
        .flat_map(|team| &mut team.slots)
        .filter_map(|slot| slot.who.as_mut())
    {
        member.me = member.account == AccountId(if owner { 118 } else { 2207 });
    }
    let mut view = logged_in(Page::Lobby, PageView::Lobby(lobby));
    view.player = if owner { "kestrel" } else { "mira" }.into();
    view.account = Some(view.player.clone());
    view.lobby_ui = ControlsView {
        modes: modes(),
        mode: Some(RulesetId("full".into())),
        chat: chat(),
        ..Default::default()
    };
    edit_preview(name, &mut view);
    view
}

fn edit_preview(name: &str, view: &mut MenuView) {
    if name == "lobby-edit" {
        view.lobby_ui.editor = Some(EditorView {
            fields: vec![
                field("Lobby name", "Friday stack", false),
                field("Password", "••••••", false),
            ],
            creating: false,
            mode: RulesetId("full".into()),
            audience: Audience::Everyone,
            cheats: Some(false),
            hidden: true,
            password_unknown: false,
            remove_password: false,
            error: None,
            saving: false,
        });
    }
}

fn chat() -> ChatView {
    let people = [
        (118, "kestrel#118"),
        (2207, "mira#2207"),
        (7, "lowtide#7"),
        (3311, "tamsin#3311"),
        (90, "owlbear#90"),
        (55, "quartz#55"),
        (4, "hookline#4"),
    ]
    .into_iter()
    .map(|(id, name)| Person {
        account: AccountId(id),
        name: name.into(),
    })
    .collect();
    let lines = [
        (1262, None, "", "tamsin#3311 joined the lobby."),
        (1262, Some(118), "kestrel#118", "two more and we start"),
        (1263, Some(90), "owlbear#90", "I'll take Dire 2"),
        (1263, None, "", "kestrel#118 set the mode to All Pick."),
        (1264, Some(2207), "mira#2207", "ready when you are"),
        (1264, Some(3311), "tamsin#3311", "gl hf"),
    ]
    .into_iter()
    .map(|(minute, account, name, message)| Line {
        minute,
        account: account.map(AccountId),
        name: name.into(),
        message: message.into(),
    })
    .collect();
    ChatView {
        owner: Some(AccountId(118)),
        people,
        lines,
        available: true,
        ..Default::default()
    }
}
