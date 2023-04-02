use std::time::Duration;

use sdl2::keyboard::Keycode;

use crate::{KeyBind, KeyMod};

/// Width of the insertion mode cursor
pub const INSERT_CURSOR_WIDTH: f32 = 0.25;
/// Width of the normal mode cursor
pub const NORMAL_CURSOR_WIDTH: f32 = 1.;
/// Maximum text size/scale
pub const MAX_SCALE: f32 = 64.;
/// How long the cursor should blink
pub const BLINK_TIME: Duration = Duration::from_millis(500);
/// Margin (in letter size) to draw around both sides
pub const MARGIN: f32 = 2.;
/// How long the scaling animation should take
pub const SCALE_ANIM_TIME: Duration = Duration::from_millis(100);
/// How long the scrolling animation should take should take
pub const SCROLL_ANIM_TIME: Duration = Duration::from_millis(100);
/// Offset from the bottom of the central line to the centre of the screen
pub const CENTER_OFFSET: f32 = -0.5;

/// Insertion mode: print available fonts to console (tmp)
pub(crate) const INSERT_PRINT_FONTS: KeyBind = KeyBind::ctrl(Keycode::F);
/// Insertion mode: cycle font
pub(crate) const INSERT_CYCLE_FONTS: KeyBind = KeyBind::new(Keycode::F, KeyMod::CtrlShift);
/// Insertion mode: copy whole buffer to system clipboard
pub(crate) const INSERT_COPY: KeyBind = KeyBind::ctrl(Keycode::C);
/// Insertion mode: paste whole buffer from system clipboard
pub(crate) const INSERT_PASTE: KeyBind = KeyBind::ctrl(Keycode::V);
