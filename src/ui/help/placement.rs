//! Place help outside its complete anchor row, using the available viewport space.
use super::Side;
use bevy::{prelude::*, window::PrimaryWindow};

#[derive(Component)]
pub(crate) struct Anchor {
    pub row: Entity,
    pub side: Side,
}

/// How far (physical pixels) a drawn box may be from the place wanted for it and still count as
/// there: the layout rounds the edges of a box to whole pixels.
pub(crate) const PLACED_WITHIN: f32 = 1.0;

/// Whether a box drawn at `actual` is at the place wanted for its top left corner, give or take
/// what the layout's rounding allows.
#[must_use]
pub(crate) fn is_placed(actual: Rect, wanted: Vec2) -> bool {
    (actual.min - wanted).abs().max_element() <= PLACED_WITHIN
}

pub(super) fn place(
    window: Single<&Window, With<PrimaryWindow>>,
    boxes: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut tips: Query<(
        &Anchor,
        &ChildOf,
        &ComputedNode,
        &UiGlobalTransform,
        &UiTransform,
        &mut Node,
        &mut Visibility,
    )>,
) {
    let extent = UVec2::new(window.physical_width(), window.physical_height()).as_vec2();
    for (anchor, parent, tip, transform, motion, mut node, mut visible) in &mut tips {
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
        // Layout applies the new offsets next frame: it is shown on the first frame it is drawn at
        // its place (the slide of its opening included), never before, at a place it had for the
        // anchor's old position or the window's old size.
        let sliding = Vec2::new(px_of(motion.translation.x), px_of(motion.translation.y)) / unit;
        if tip.size().min_element() <= 0.0
            || !is_placed(actual, position + sliding)
            || !clear(actual, row, 4.0 / unit)
            || !contained(actual, Rect::from_corners(Vec2::ZERO, extent))
        {
            visible.set_if_neq(Visibility::Hidden);
        }
    }
}

/// The interface pixels of a length that is one; the slide of a popup is always in them.
fn px_of(length: Val) -> f32 {
    match length {
        Val::Px(pixels) => pixels,
        _ => 0.0,
    }
}

fn contained(rect: Rect, bounds: Rect) -> bool {
    rect.min.cmpge(bounds.min).all() && rect.max.cmple(bounds.max).all()
}

fn clear(rect: Rect, row: Rect, gap: f32) -> bool {
    (row.min - rect.max).max(rect.min - row.max).max_element() >= gap
}

/// The four places beside `row`, a `gap` off it, in the order of preference of `preferred`: a box
/// of `size` above and below the row stands as near `point` as the bounds allow, left and right
/// level with it.
fn sides(row: Rect, size: Vec2, point: Vec2, bounds: Rect, gap: f32, preferred: Side) -> [Vec2; 4] {
    let x = (point.x - size.x / 2.0).clamp(bounds.min.x, (bounds.max.x - size.x).max(bounds.min.x));
    let y = (point.y - size.y / 2.0).clamp(bounds.min.y, (bounds.max.y - size.y).max(bounds.min.y));
    let left = Vec2::new(row.min.x - gap - size.x, y);
    let right = Vec2::new(row.max.x + gap, y);
    let above = Vec2::new(x, row.min.y - gap - size.y);
    let below = Vec2::new(x, row.max.y + gap);
    match preferred {
        Side::Above => [above, below, left, right],
        Side::Left => [left, right, above, below],
        Side::Right => [right, left, above, below],
        Side::Below => [below, above, left, right],
    }
}

/// Where a box of `size` stands clear of `row`, inside `bounds`: the preferred side, else the next
/// that has room. None when no side has room, or the box has no size yet.
#[must_use]
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
    sides(row, size, point, bounds, gap, preferred)
        .into_iter()
        .find(|point| contained(Rect::from_corners(*point, *point + size), bounds))
}

/// [`position`], and where no side has room, the side where the box covers `row` least, pushed
/// inside `bounds`; a box larger than `bounds` stands at its top left corner and is cut at the
/// right and the bottom. For what must be shown whatever the room: a tooltip is for its owner,
/// and an owner whose tooltip cannot be placed has none. None only for a box with no size yet.
#[must_use]
pub(crate) fn position_within(
    row: Rect,
    size: Vec2,
    point: Vec2,
    bounds: Rect,
    gap: f32,
    preferred: Side,
) -> Option<Vec2> {
    if size.min_element() <= 0.0 {
        return None;
    }
    if let Some(clear) = position(row, size, point, bounds, gap, preferred) {
        return Some(clear);
    }
    // The top left corner stays inside when the box is larger than the bounds: `max` after `min`.
    let pushed = |at: Vec2| (at.min(bounds.max - size)).max(bounds.min);
    let covered = |at: Vec2| {
        let both = Rect::from_corners(at, at + size).intersect(row);
        both.width() * both.height()
    };
    // The first of the best, so that the order of preference breaks ties.
    sides(row, size, point, bounds, gap, preferred)
        .into_iter()
        .map(pushed)
        .reduce(|best, next| {
            if covered(next) < covered(best) {
                next
            } else {
                best
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Rect = Rect {
        min: Vec2::new(8.0, 8.0),
        max: Vec2::new(1016.0, 592.0),
    };

    /// A tooltip as tall as the free space, beside an owner in the middle of a small window: no side
    /// clears the owner, and the tooltip is shown regardless.
    #[test]
    fn a_tooltip_with_no_side_clear_of_its_owner_is_placed_inside_the_window_beside_it() {
        let owner = Rect::from_center_size(Vec2::new(500.0, 300.0), Vec2::new(60.0, 60.0));
        let size = Vec2::new(700.0, 500.0);
        assert_eq!(
            position(owner, size, owner.center(), WINDOW, 12.0, Side::Above),
            None,
            "no side is clear of the owner: the strict rule shows nothing"
        );
        let at = position_within(owner, size, owner.center(), WINDOW, 12.0, Side::Above)
            .expect("a tooltip of a laid-out size always has a place");
        assert!(at.cmpge(WINDOW.min).all(), "{at} is inside the window");
        assert!(
            (at + size).cmple(WINDOW.max).all(),
            "{at} is inside the window"
        );
    }

    /// Taller than the window: the top left corner is inside, the bottom is cut.
    #[test]
    fn a_tooltip_larger_than_the_window_stands_at_its_top_left_corner() {
        let owner = Rect::from_center_size(Vec2::new(500.0, 300.0), Vec2::new(60.0, 60.0));
        let size = Vec2::new(300.0, 900.0);
        let at = position_within(owner, size, owner.center(), WINDOW, 12.0, Side::Above)
            .expect("a tooltip of a laid-out size always has a place");
        assert_eq!(
            at.y, WINDOW.min.y,
            "the top edge is inside, the bottom is cut"
        );
        assert!(at.x >= WINDOW.min.x && at.x + size.x <= WINDOW.max.x);
    }

    /// With a side clear of the owner, the preferred side wins as it did.
    #[test]
    fn a_tooltip_with_room_stands_clear_of_its_owner_on_the_preferred_side() {
        let owner = Rect::from_center_size(Vec2::new(500.0, 500.0), Vec2::new(60.0, 60.0));
        let size = Vec2::new(200.0, 100.0);
        let at =
            position_within(owner, size, owner.center(), WINDOW, 12.0, Side::Above).expect("room");
        assert_eq!(
            Some(at),
            position(owner, size, owner.center(), WINDOW, 12.0, Side::Above)
        );
        assert!(at.y + size.y + 12.0 <= owner.min.y, "above the owner");
    }

    /// Without a clear side, the one that covers the owner least wins: here the right, which the
    /// window's edge pushes back over 144 of the owner's 200 columns, where the left covers 168.
    #[test]
    fn a_tooltip_with_no_clear_side_covers_its_owner_the_least() {
        let owner = Rect::from_center_size(Vec2::new(500.0, 300.0), Vec2::new(200.0, 200.0));
        let size = Vec2::new(560.0, 400.0);
        assert_eq!(
            position(owner, size, owner.center(), WINDOW, 12.0, Side::Above),
            None,
            "no side is clear of the owner"
        );
        let at = position_within(owner, size, owner.center(), WINDOW, 12.0, Side::Above)
            .expect("a place");
        assert_eq!(
            at.x,
            WINDOW.max.x - size.x,
            "against the window's right edge: {at}"
        );
    }
}
