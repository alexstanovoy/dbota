//! Place help outside its complete anchor row, using the available viewport space.
use super::Side;
use bevy::{prelude::*, window::PrimaryWindow};

#[derive(Component)]
pub(super) struct Anchor {
    pub row: Entity,
    pub side: Side,
}

pub(super) fn place(
    window: Single<&Window, With<PrimaryWindow>>,
    boxes: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut tips: Query<(
        &Anchor,
        &ChildOf,
        &ComputedNode,
        &UiGlobalTransform,
        &mut Node,
        &mut Visibility,
    )>,
) {
    let extent = UVec2::new(window.physical_width(), window.physical_height()).as_vec2();
    for (anchor, parent, tip, transform, mut node, mut visible) in &mut tips {
        if node.display == Display::None {
            continue;
        }
        let (Ok((row, row_at)), Ok((owner, owner_at))) =
            (boxes.get(anchor.row), boxes.get(parent.parent()))
        else {
            continue;
        };
        let unit = tip.inverse_scale_factor();
        if unit <= 0.0 {
            continue;
        }
        let bounds = Rect::from_corners(Vec2::splat(8.0 / unit), extent - Vec2::splat(8.0 / unit));
        let width = anchor.side.width().min(bounds.width().max(1.0) * unit);
        if node.width != px(width) {
            node.width = px(width);
        }
        let row = Rect::from_center_size(row_at.translation, row.size());
        let actual = Rect::from_center_size(transform.translation, tip.size());
        let Some(position) = position(
            row,
            tip.size(),
            owner_at.translation,
            bounds,
            12.0 / unit,
            anchor.side,
        ) else {
            visible.set_if_neq(Visibility::Hidden);
            continue;
        };
        let Some(inverse) = owner_at.try_inverse() else {
            continue;
        };
        let local = (inverse.transform_point2(position) - owner.padding_box().min) * unit;
        if node.left != px(local.x) {
            node.left = px(local.x);
        }
        if node.top != px(local.y) {
            node.top = px(local.y);
        }
        // Layout applies the new offsets next frame. Never expose the previous overlapping box.
        if tip.size().min_element() <= 0.0
            || !clear(actual, row, 4.0 / unit)
            || !contained(actual, Rect::from_corners(Vec2::ZERO, extent))
        {
            visible.set_if_neq(Visibility::Hidden);
        }
    }
}

fn contained(rect: Rect, bounds: Rect) -> bool {
    rect.min.cmpge(bounds.min).all() && rect.max.cmple(bounds.max).all()
}

fn clear(rect: Rect, row: Rect, gap: f32) -> bool {
    (row.min - rect.max).max(rect.min - row.max).max_element() >= gap
}

pub(crate) fn position(
    row: Rect,
    size: Vec2,
    point: Vec2,
    bounds: Rect,
    gap: f32,
    preferred: Side,
) -> Option<Vec2> {
    if size.min_element() <= 0.0 || size.x > bounds.width() || size.y > bounds.height() {
        return None;
    }
    let x = (point.x - size.x / 2.0).clamp(bounds.min.x, bounds.max.x - size.x);
    let y = (point.y - size.y / 2.0).clamp(bounds.min.y, bounds.max.y - size.y);
    let left = Vec2::new(row.min.x - gap - size.x, y);
    let right = Vec2::new(row.max.x + gap, y);
    let above = Vec2::new(x, row.min.y - gap - size.y);
    let below = Vec2::new(x, row.max.y + gap);
    let candidates = match preferred {
        Side::Above => [above, below, left, right],
        Side::Left => [left, right, above, below],
        Side::Right => [right, left, above, below],
        Side::Below => [below, above, left, right],
    };
    candidates
        .into_iter()
        .find(|point| contained(Rect::from_corners(*point, *point + size), bounds))
}
