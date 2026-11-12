//! Independent lobby panes retain their positions when their nodes are redrawn.
use super::{MenuCore, core::Page, lobby_controls::Choice, shell::Preview, view::MenuView};
use bevy::{
    input::mouse::{AccumulatedMouseScroll, MouseScrollUnit},
    prelude::*,
    window::PrimaryWindow,
};

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    Browser,
    Choices,
    Radiant,
    Dire,
    Chat,
    People,
    Spectators,
}

#[derive(Resource)]
pub struct Positions {
    offsets: [f32; 7],
    context: Option<(Page, Option<bota_radovid::proto::LobbyId>)>,
    choice: Option<Choice>,
    choice_start: usize,
    follow_chat: bool,
}
impl Default for Positions {
    fn default() -> Self {
        Self {
            offsets: [0.0; 7],
            context: None,
            choice: None,
            choice_start: 0,
            follow_chat: true,
        }
    }
}

impl Positions {
    fn prepare(&mut self, view: &MenuView, lobby: Option<bota_radovid::proto::LobbyId>) -> bool {
        if !matches!(view.beneath, Page::Lobbies | Page::Lobby) {
            *self = Self::default();
            return false;
        }
        let context = (view.beneath, lobby);
        if self.context != Some(context) {
            *self = Self {
                context: Some(context),
                ..Default::default()
            };
        }
        if self.choice != view.lobby_ui.open {
            self.choice = view.lobby_ui.open;
            self.offsets[Area::Choices as usize] = 0.0;
            self.choice_start = 0;
        }
        if self.choice_start != view.lobby_ui.choice_start {
            self.choice_start = view.lobby_ui.choice_start;
            self.offsets[Area::Choices as usize] =
                self.choice_start as f32 * crate::theme::lobby::CHOICE_ROW_HEIGHT;
        }
        true
    }

    fn move_pane(&mut self, area: Area, reach: f32, step: f32) -> f32 {
        let index = area as usize;
        if area == Area::Chat && self.follow_chat {
            self.offsets[index] = reach;
        }
        self.offsets[index] = (self.offsets[index] - step).clamp(0.0, reach);
        if area == Area::Chat && step != 0.0 {
            self.follow_chat = self.offsets[index] >= reach;
        }
        self.offsets[index]
    }
}

type Panes<'w, 's> = Query<
    'w,
    's,
    (
        &'static Area,
        &'static mut ScrollPosition,
        &'static ComputedNode,
        &'static UiGlobalTransform,
    ),
>;

fn pointer_pane(view: &MenuView, cursor: Option<Vec2>, panes: &mut Panes) -> Option<Area> {
    let cursor = cursor?;
    let mut target = None;
    for (area, _, node, at) in panes {
        if view.lobby_ui.editor.is_some() && *area != Area::Choices {
            continue;
        }
        if Rect::from_center_size(at.translation, node.size()).contains(cursor) {
            if *area == Area::Choices {
                return Some(*area);
            }
            target = Some(*area);
        }
    }
    target
}

fn reach(area: Area, node: &ComputedNode) -> f32 {
    let extent = if area == Area::Spectators {
        node.content_size().x - node.size().x
    } else {
        node.content_size().y - node.size().y
    };
    (extent * node.inverse_scale_factor()).max(0.0)
}

#[allow(clippy::too_many_arguments)]
pub fn scroll(
    mut core: Option<NonSendMut<MenuCore>>,
    mut preview: Option<ResMut<Preview>>,
    wheel: Res<AccumulatedMouseScroll>,
    windows: Query<&Window, With<PrimaryWindow>>,
    settings: Res<crate::settings::Screen>,
    mut positions: ResMut<Positions>,
    mut panes: Panes,
) {
    let view = match (&preview, &core) {
        (Some(preview), _) => preview.0.clone(),
        (None, Some(core)) => core.view(),
        _ => return,
    };
    let lobby = core
        .as_ref()
        .and_then(|core| core.lobby.as_ref().map(|(state, _)| state.id));
    if !positions.prepare(&view, lobby) {
        return;
    }
    let blocked =
        settings.open || view.offer.is_some() || view.confirm.is_some() || view.password.is_some();
    let cursor = windows.single().ok().and_then(|window| {
        window
            .cursor_position()
            .map(|at| at * window.scale_factor())
    });
    let target = if blocked {
        None
    } else {
        pointer_pane(&view, cursor, &mut panes)
    };
    let step = wheel.delta.y
        * if wheel.unit == MouseScrollUnit::Line {
            40.0
        } else {
            1.0
        };
    let choice_start = move_panes(&mut positions, &mut panes, target, step);
    if let Some(start) = choice_start {
        positions.choice_start = start;
        if let Some(preview) = preview.as_mut() {
            preview.0.lobby_ui.choice_start = start;
        } else if let Some(core) = core.as_mut() {
            core.lobby_ui.choice_start = start;
        }
    }
}

fn move_panes(
    positions: &mut Positions,
    panes: &mut Panes,
    target: Option<Area>,
    step: f32,
) -> Option<usize> {
    let mut choice_start = None;
    for (area, mut at, node, _) in panes {
        if node.size().y <= 0.0 {
            continue;
        }
        let step = if target == Some(*area) { step } else { 0.0 };
        let position = positions.move_pane(*area, reach(*area, node), step);
        if *area == Area::Spectators {
            at.x = position;
        } else {
            at.y = position;
        }
        if *area == Area::Choices {
            choice_start = Some((position / crate::theme::lobby::CHOICE_ROW_HEIGHT) as usize);
        }
    }
    choice_start
}
