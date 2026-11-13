//! Modal input and redraw: the inspected match never changes the running session.
use super::{
    Origin, PostGame,
    render::{self, primitives::Hover},
    view::{Action, View},
};
use crate::{
    main_menu::{MenuCore, kit::Kit},
    session::Session,
};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use std::sync::Arc;

pub fn present(
    mut post: NonSendMut<PostGame>,
    mut core: Option<NonSendMut<MenuCore>>,
    session: NonSend<Session>,
) {
    if let Some(core) = core.as_mut() {
        if let Some(view) = core.take_result_request(
            post.overlay
                .as_ref()
                .is_some_and(|view| view.context.history),
        ) {
            if !view.context.history {
                post.closed = false;
                post.outcome_revision = core.outcome_revision();
                post.active_version = post.version;
            }
            post.overlay = Some(view);
            post.presented += 1;
            return;
        }
        let revision = core.outcome_revision();
        if post.shown.is_none()
            && !post.closed
            && revision != post.outcome_revision
            && !post
                .overlay
                .as_ref()
                .is_some_and(|view| view.context.history)
        {
            post.outcome_revision = revision;
            if let Some(mut view) = core.outcome_result() {
                if let Some(old) = &post.overlay {
                    view.select(old.page);
                    view.metric = old.metric;
                    view.player = old.player.filter(|slot| view.row(*slot).is_some());
                    view.focus = old.focus.filter(|slot| view.row(*slot).is_some());
                    view.picker = old.picker;
                    view.notice = old.notice.clone();
                }
                post.overlay = Some(view);
                post.presented += 1;
                return;
            }
        }
        if let Some(view) = post.overlay.as_mut().filter(|view| !view.context.history) {
            let mut context = core.result_context(&view.summary, view.context.owner);
            let header = matches!(
                context.kind,
                super::view::Kind::Ranked | super::view::Kind::Unranked
            ) && !view.context.same_header(&context);
            let replay = view.context.replay.is_none() && context.replay.is_some();
            if header || replay {
                context.replay = view.context.replay.clone().or(context.replay);
                if header {
                    view.context = context;
                } else {
                    view.context.replay = context.replay;
                }
                post.presented += 1;
            }
        }
    }
    if post.version == post.active_version || post.closed {
        return;
    }
    if post
        .overlay
        .as_ref()
        .is_some_and(|view| view.context.history)
    {
        return;
    }
    if let Some(summary) = &post.shown {
        let summary = Arc::new(summary.clone());
        let mut context = core.as_ref().map_or_else(Default::default, |core| {
            core.result_context(
                &summary,
                if matches!(session.0.source, bota_client_core::Source::Live(_)) {
                    session.0.my_slot
                } else {
                    None
                },
            )
        });
        if summary.recording.is_none()
            && context.kind == super::view::Kind::Direct
            && let Some(map) = session.0.session().map(|session| session.map().id)
        {
            context.mode = if map.0 == 2 { "Solo Mid" } else { "All Pick" }.to_owned();
        }
        context.replay = match &post.origin {
            Origin::File(path) => {
                #[cfg(not(target_arch = "wasm32"))]
                let kept = core
                    .as_ref()
                    .is_none_or(|core| core.recording_is_kept(path));
                #[cfg(target_arch = "wasm32")]
                let kept = true;
                kept.then(|| super::view::Replay::File(path.clone()))
            }
            Origin::Blob(bytes) => Some(super::view::Replay::Blob(bytes.clone())),
            _ => context.replay.take(),
        };
        let mut view = View::new(summary, context);
        if let Some(old) = &post.overlay {
            view.select(old.page);
        }
        post.overlay = Some(view);
    } else {
        post.overlay = None;
    }
    post.active_version = post.version;
    post.presented += 1;
}
fn apply(post: &mut PostGame, core: Option<&mut MenuCore>, action: Action) {
    let Some(view) = post.overlay.as_mut() else {
        return;
    };
    if view.notice.is_some()
        && !matches!(action, Action::Dismiss | Action::Close | Action::Continue)
    {
        return;
    }
    match action {
        Action::Dismiss => view.notice = None,
        Action::Close | Action::Continue => {
            if !view.context.history {
                if let Some(core) = core {
                    core.return_from_results();
                }
                post.closed = true;
            }
            post.overlay = None;
        }
        Action::Watch => {
            if let Some(replay) = view.context.replay.clone()
                && let Some(core) = core
            {
                match core.watch_result(replay) {
                    Ok(()) => {
                        post.overlay = None;
                        post.closed = true;
                    }
                    Err(failure) => view.notice = Some(failure.message().to_owned()),
                }
            }
        }
        Action::Previous => view.advance(false),
        Action::Next => view.advance(true),
        Action::Page(page) => view.select(page),
        Action::Player(slot) => view.open_player(slot),
        Action::Back => {
            view.player = None;
            view.select(super::view::Page::Scoreboard);
        }
        Action::Picker => view.picker = !view.picker,
        Action::Metric(metric) => {
            view.metric = metric;
            if metric == super::graphs::Metric::Advantage {
                view.focus = None;
            }
            view.picker = false;
        }
        Action::Focus(slot) => {
            view.focus = (view.focus != Some(slot)).then_some(slot);
            if view.metric == super::graphs::Metric::Advantage {
                view.metric = super::graphs::Metric::NetWorth;
            }
            view.picker = false;
        }
    }
    post.presented = post.presented.wrapping_add(1);
}
pub fn keyboard(
    mut post: NonSendMut<PostGame>,
    mut core: Option<NonSendMut<MenuCore>>,
    mut fired: ResMut<crate::keys::Fired>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut events: ResMut<Messages<bevy::input::keyboard::KeyboardInput>>,
) {
    if !post.visible() {
        return;
    }
    events.clear();
    let escape = crate::keys::Action::Escape;
    let action = if fired.pressed(escape) {
        fired.pressed.retain(|key| *key != escape);
        Some(
            if post
                .overlay
                .as_ref()
                .is_some_and(|view| view.notice.is_some())
            {
                Action::Dismiss
            } else if post.overlay.as_ref().is_some_and(|view| view.picker) {
                Action::Picker
            } else if post
                .overlay
                .as_ref()
                .is_some_and(|view| view.player.is_some())
            {
                Action::Back
            } else {
                Action::Close
            },
        )
    } else if keys.just_pressed(KeyCode::ArrowLeft) {
        Some(Action::Previous)
    } else if keys.just_pressed(KeyCode::ArrowRight) {
        Some(Action::Next)
    } else {
        None
    };
    keys.reset_all();
    if let Some(action) = action {
        apply(&mut post, core.as_deref_mut(), action);
    }
}
pub fn clicks(
    mut post: NonSendMut<PostGame>,
    mut core: Option<NonSendMut<MenuCore>>,
    buttons: Query<&super::view::Button>,
    press: Res<crate::ui::PrimaryPress>,
    mouse: Res<ButtonInput<MouseButton>>,
    regions: Query<&bevy::ui::RelativeCursorPosition, With<render::chart::PickerRegion>>,
) {
    if let Some(button) = press.0.and_then(|target| buttons.get(target).ok()) {
        apply(&mut post, core.as_deref_mut(), button.0.clone());
    } else if mouse.just_pressed(MouseButton::Left)
        && post.overlay.as_ref().is_some_and(|view| view.picker)
        && regions.iter().all(|region| !region.cursor_over)
    {
        apply(&mut post, core.as_deref_mut(), Action::Picker);
    }
}

#[derive(Component)]
pub struct Root;
#[allow(clippy::too_many_arguments)]
pub fn redraw(
    commands: Commands,
    post: NonSend<PostGame>,
    core: Option<NonSend<MenuCore>>,
    fonts: Res<crate::ui::Fonts>,
    manifest: Res<crate::manifest::Manifest>,
    server: Res<AssetServer>,
    screens: Query<Entity, With<Root>>,
    mut drawn: Local<Option<u32>>,
) {
    if *drawn == Some(post.presented) {
        return;
    }
    *drawn = Some(post.presented);
    let mut kit = Kit {
        commands,
        fonts: &fonts,
        manifest: &manifest,
        server: &server,
    };
    for screen in &screens {
        kit.commands.entity(screen).despawn();
    }
    if let Some(view) = &post.overlay {
        let root = kit
            .commands
            .spawn((Root, crate::ui::fill(), GlobalZIndex(80)))
            .id();
        if !view.context.history
            && let Some(core) = core
        {
            crate::main_menu::results::shell(&mut kit, root, &core);
        }
        render::spawn(&mut kit, root, view);
    }
}
pub fn hover(mut buttons: Query<(&Hovered, &Hover, &mut BackgroundColor), Changed<Hovered>>) {
    for (hovered, hover, mut colour) in &mut buttons {
        colour.0 = if hovered.get() {
            hover.over
        } else {
            hover.rest
        };
    }
}
pub fn focus_outline(
    focus: Res<bevy::input_focus::InputFocus>,
    mut buttons: Query<(Entity, &mut Outline), With<super::view::Button>>,
) {
    if !focus.is_changed() {
        return;
    }
    for (entity, mut outline) in &mut buttons {
        outline.color = if focus.get() == Some(entity) {
            crate::theme::FOCUS
        } else {
            Color::NONE
        };
    }
}

pub fn scroll(
    post: NonSend<PostGame>,
    wheel: Res<bevy::input::mouse::AccumulatedMouseScroll>,
    parents: Query<&ChildOf>,
    screens: Query<(), With<render::Screen>>,
    mut lists: Query<
        (
            Entity,
            &bevy::ui::RelativeCursorPosition,
            &mut ScrollPosition,
            &ComputedNode,
        ),
        With<crate::ui::ScrollArea>,
    >,
) {
    let step = match wheel.unit {
        bevy::input::mouse::MouseScrollUnit::Line => wheel.delta.y * 36.0,
        _ => wheel.delta.y,
    };
    for (entity, cursor, mut position, node) in &mut lists {
        let in_modal = std::iter::successors(Some(entity), |entity| {
            parents.get(*entity).ok().map(ChildOf::parent)
        })
        .take(32)
        .any(|entity| screens.contains(entity));
        if !cursor.cursor_over || (post.visible() && !in_modal) {
            continue;
        }
        let reach =
            ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0);
        position.y = (position.y - step).clamp(0.0, reach);
    }
}

/// A departed row stays readable but recedes as one subtree, including icons and hover.
pub fn dim(
    mut commands: Commands,
    roots: Query<Entity, Added<render::Dim>>,
    children: Query<&Children>,
    mut visuals: Query<(
        Option<&mut TextColor>,
        Option<&mut ImageNode>,
        Option<&mut BackgroundColor>,
        Option<&mut BorderColor>,
        Option<&mut Hover>,
    )>,
) {
    for root in &roots {
        let mut pending = vec![root];
        while let Some(entity) = pending.pop() {
            if let Ok(descendants) = children.get(entity) {
                pending.extend(descendants.iter());
            }
            if let Ok((text, image, background, border, hover)) = visuals.get_mut(entity) {
                let fade = |colour: Color| colour.with_alpha(colour.alpha() * 0.6);
                if let Some(mut text) = text {
                    text.0 = fade(text.0);
                }
                if let Some(mut image) = image {
                    image.color = fade(image.color);
                }
                if let Some(mut background) = background {
                    background.0 = fade(background.0);
                }
                if let Some(mut border) = border {
                    border.top = fade(border.top);
                    border.bottom = fade(border.bottom);
                    border.left = fade(border.left);
                    border.right = fade(border.right);
                }
                if let Some(mut hover) = hover {
                    hover.rest = fade(hover.rest);
                    hover.over = fade(hover.over);
                }
            }
        }
        commands.entity(root).remove::<render::Dim>();
    }
}
