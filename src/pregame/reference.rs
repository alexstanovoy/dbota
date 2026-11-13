//! The lengths of the original game's pick and strategy screens, measured by hand in two
//! reference screenshots of the game, in the screenshots' own pixels. Numbers only: no
//! picture or pixel of a reference is kept anywhere in the repository.
//!
//! The screens are drawn from these through [`super::Shot`]. The render tests
//! (`e2e::pregame_layout`) do not read this table: they hold the measurements again as literals of
//! their own, so a changed number here is caught.

/// The pick screenshot: 2868 by 1618.
pub const PICK_HEIGHT: f32 = 1618.0;
/// The strategy screenshot: 2872 by 1611.
pub const STRATEGY_WIDTH: f32 = 2872.0;
pub const STRATEGY_HEIGHT: f32 = 1611.0;

// ---- Both screens: the bar of seats along the top ----

/// A seat's box (width, height) and the distance between two seats' left edges.
pub const SEAT_SIZE: [f32; 2] = [176.0, 104.0];
pub const SEAT_PITCH: f32 = 186.0;
/// Radiant's first seat's left edge and Dire's first, from the screen's middle.
pub const RADIANT_SEAT_FROM_MIDDLE: f32 = -1122.0;
pub const DIRE_SEAT_FROM_MIDDLE: f32 = 206.0;
/// Seats a side's row holds (Dota's five); the row is as wide as five seats and a side with
/// fewer seats keeps them nearest the clock.
pub const SEATS_PER_SIDE: usize = 5;
/// The colour bar of a seat of the pick screen: its top in the seat's box, and its height.
pub const SEAT_BAR: [f32; 2] = [94.0, 8.0];
/// The colour bar of a seat of the strategy screen sits on the seat's top.
pub const STRATEGY_SEAT_BAR_HEIGHT: f32 = 9.0;
pub const STRATEGY_SEAT_HEIGHT: f32 = 110.0;
/// The name under a seat: its top in the screen, and the size of its letters.
pub const SEAT_NAME: [f32; 2] = [114.0, 22.0];
/// The clock: its top, the height of its box and its letters' size; the mode's line under it.
pub const CLOCK: [f32; 3] = [4.0, 88.0, 96.0];
pub const CLOCK_WIDTH: f32 = 398.0;
/// The strategy screen's clock: its top, the height of its box and its letters' size.
pub const STRATEGY_CLOCK: [f32; 3] = [14.0, 88.0, 84.0];
pub const CLOCK_CAPTION: f32 = 30.0;

// ---- The pick screen ----

/// "CHOOSE YOUR HERO": the top of its line, the letters' size and their spacing.
pub const HEADING: [f32; 3] = [208.0, 44.0, 7.0];
/// A group of heroes: the left of the first, the distance between two, the top of the header
/// row, and its height.
pub const GROUP_LEFT: f32 = 192.0;
pub const GROUP_PITCH: f32 = 481.0;
pub const GROUP_HEADER: [f32; 2] = [268.0, 26.0];
/// The heroes' cards: their top in the screen, their box, their pitch and how many make a row.
pub const CARDS_TOP: f32 = 310.0;
pub const CARD: [f32; 2] = [64.0, 112.0];
pub const CARD_PITCH: [f32; 2] = [77.0, 125.0];
pub const CARD_COLUMNS: usize = 6;
/// Cards stand this far in from the group's header's left.
pub const CARD_INSET: f32 = 5.0;
/// The hero panel at the right.
pub const PANEL: [f32; 3] = [2012.0, 268.0, 564.0];
/// The panel's height: the space under the bars is the reference's description and similar
/// heroes, left empty.
pub const PANEL_HEIGHT: f32 = 907.0;
pub const PANEL_TITLE_HEIGHT: f32 = 67.0;
pub const PANEL_INSET: f32 = 26.0;
pub const PANEL_PORTRAIT: [f32; 2] = [238.0, 366.0];
pub const PANEL_ATTRIBUTES: [f32; 2] = [274.0, 190.0];
pub const PANEL_STATS: [f32; 2] = [274.0, 176.0];
/// The row of skills: the round marks' side and the gap after each (talents, innate), then the
/// abilities' square and the distance between two.
pub const PANEL_MARK: f32 = 50.0;
pub const PANEL_MARKS_GAP: f32 = 6.0;
pub const PANEL_SKILL: f32 = 54.0;
pub const PANEL_SKILL_PITCH: f32 = 58.0;
pub const PANEL_SKILLS_GAP: f32 = 14.0;
pub const PANEL_BAR: [f32; 2] = [512.0, 38.0];
pub const PANEL_BARS_GAP: f32 = 38.0;
pub const PANEL_BAR_PITCH: f32 = 42.0;
/// LOCK IN and RANDOM: widths, height and the gap between them and under the panel.
pub const LOCK_IN: [f32; 2] = [406.0, 98.0];
pub const RANDOM: [f32; 2] = [140.0, 98.0];
pub const BUTTONS_GAP: [f32; 2] = [18.0, 12.0];

// ---- The strategy screen ----

/// ENTERING BATTLE AS and the hero's name: the left of both, the top of the first line, and the
/// letters' sizes.
pub const ENTERING: [f32; 4] = [265.0, 196.0, 24.0, 62.0];
pub const ENTERING_ATTRIBUTE: f32 = 56.0;
/// The row of the hero's abilities: its left, its top, the icons' side and the gap between
/// two; the round marks' (talents, innate) side and the gap between them, and the gap after them.
pub const ABILITY_ROW_LEFT: f32 = 262.0;
pub const ABILITIES: [f32; 3] = [315.0, 66.0, 8.0];
pub const ABILITY_MARK: f32 = 58.0;
pub const MARKS_GAP: f32 = 15.0;
pub const ABILITIES_GAP: f32 = 20.0;
/// The hero's model: the share of the screen's height it stands tall, where its feet stand, and
/// the middle of it across, as a share of the screen's width.
pub const MODEL: [f32; 3] = [0.57, 0.895, 0.237];
/// STARTING ITEMS: its left and width, and its top (the first block of the right column).
pub const STARTING_ITEMS: [f32; 3] = [1246.0, 247.0, 1323.0];
