extern crate sdl2;

use atlas::GlyphAtlas;
use gl::types::GLfloat;
use sdl2::clipboard::ClipboardUtil;
use sdl2::event::Event;
use sdl2::keyboard::{Keycode, Mod};
use sdl2::EventPump;
use shader::Shader;

use std::mem::replace;
use std::time::{Duration, Instant};
use std::{iter, ptr};

/*  To Do
   To do eventually
   - Reset cursor timer on every keystroke
   - Add font picker
*/

mod atlas;
mod config;
mod rope;
mod shader;
use config::*;

macro_rules! log_err {
    ($e:expr) => {
        let e = $e;
        if let Err(e) = e {
            eprintln!("{}", e);
        }
    };
}

fn round_to_scale(value: f32, scale: f32) -> f32 {
    const C: f32 = 1.;
    (value * C * scale).round() / (C * scale)
}

pub fn main() {
    let sdl_context = sdl2::init().unwrap();
    let video_subsystem = sdl_context.video().unwrap();
    video_subsystem.text_input().start();

    let clipboard = video_subsystem.clipboard();

    let mut window = video_subsystem
        .window("", 800, 600)
        .opengl()
        .position_centered()
        .resizable()
        .build()
        .unwrap();

    let _ctx = window.gl_create_context().unwrap();
    // bye bye vsync
    video_subsystem.gl_set_swap_interval(0).unwrap();
    gl::load_with(|name| video_subsystem.gl_get_proc_address(name).cast());

    let mut event_pump = sdl_context.event_pump().unwrap();

    // setup blending
    unsafe {
        gl::Enable(gl::BLEND);
        gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
    };

    let vbo = unsafe {
        let mut vbo = 0;
        gl::GenBuffers(1, &mut vbo);
        check_err();
        vbo
    };

    let text_shader = Shader::text_shader(vbo);
    let shape_shader = Shader::shape_shader(vbo);

    let mut screen_size = window.drawable_size();
    // Buffer for the text edited on screen. Lines are \n terminated
    let mut atlas = GlyphAtlas::new(&text_shader);
    let mut last_recorded_frame = 0;

    let mut logic_state = LogicState {
        exit: false,
        text_buffer: String::new(),
        cursor_col: 0,
        cursor_row: 0,
        line_count: 0,
        mode: EditorMode::Normal,
    };

    let mut gfx_state = GraphicsState {
        camera_scale: MAX_SCALE,
        cursor_visible: false,
        center_y: CENTER_OFFSET,
    };

    let mut scale_animation = TimeInterpolator::new(gfx_state.camera_scale, SCALE_ANIM_TIME);
    let mut scroll_animation = TimeInterpolator::new(gfx_state.center_y, SCROLL_ANIM_TIME);

    let run_timer = Instant::now();
    let mut frame_timer = Instant::now();
    'running: for frame_counter in 0.. {
        // fps tracking
        if frame_timer.elapsed().as_secs_f32() >= 0.5 {
            let elapsed_frames = frame_counter - last_recorded_frame;
            let fps = elapsed_frames as f32 / frame_timer.elapsed().as_secs_f32();
            window
                .set_title(&format!("Saphedit, fps={fps:.0}"))
                .expect("String has no null bytes");
            last_recorded_frame = frame_counter;
            frame_timer = Instant::now();
        }

        let new_state = match logic_state.mode {
            EditorMode::Insert => handle_events_insert(&mut event_pump, &logic_state, &clipboard),
            EditorMode::Normal => handle_events_normal(&mut event_pump, &logic_state),
        };

        if new_state.exit {
            break 'running;
        }

        // Make sure to invalidate `new_state` as soon as possible to avoid
        // accidentally using the wrong state
        let logic_state_updated = logic_state != new_state;
        let row_moved = logic_state.cursor_row != new_state.cursor_row;
        logic_state = new_state;

        // Update screen size
        let new_screen_size = window.drawable_size();
        let resize = new_screen_size != screen_size;
        screen_size = new_screen_size;

        // Update text size / update scale
        if logic_state_updated || resize {
            let (text_w, text_h) = logic_state
                .text_buffer
                .split('\n')
                .map(|line| atlas.measure_dims(line.chars()))
                .reduce(|(w1, h1), (w2, h2)| (w1.max(w2), h1.max(h2)))
                .unwrap_or((1., 1.));
            let scale_x = new_screen_size.0 as f32 / (text_w + 2. * MARGIN);
            let scale_y = new_screen_size.1 as f32 / text_h;
            // TODO: do a better estimate of the size; the issue here is that
            // the theoretical scale depends on the text size, which can change
            // from one scale to another
            let new_scale_raw = scale_x.min(scale_y).clamp(8., MAX_SCALE);
            let step = GlyphAtlas::SCALE_STEP;
            let new_scale_rounded = (new_scale_raw / step).floor() * step;
            scale_animation.reset(new_scale_rounded);
        }

        let camera_scale = scale_animation.interpolated_value();

        atlas.select_scale(camera_scale);

        // Cursor update
        let time_period = (run_timer.elapsed().as_secs_f32() / BLINK_TIME.as_secs_f32()) as u32;
        let cursor_visible = time_period % 2 == 0;

        // Scroll update
        if row_moved {
            let y_center_new_target =
                logic_state.cursor_row as f32 * atlas.line_height() + CENTER_OFFSET;
            scroll_animation.reset(y_center_new_target);
        }

        let center_y_raw = scroll_animation.interpolated_value();
        let center_y = round_to_scale(center_y_raw, camera_scale);

        let new_gfx_state = GraphicsState {
            camera_scale,
            cursor_visible,
            center_y,
        };

        // Shouldn't get rid of the previous one
        let prev_gfx_state = replace(&mut gfx_state, new_gfx_state);

        // Dear Princess Celestia
        // I fucking hate indentation
        // Your faithful student
        // Twinkle Springle
        if gfx_state == prev_gfx_state && !logic_state_updated {
            continue;
        }

        unsafe {
            let (width, height) = screen_size;
            gl::Viewport(0, 0, width as i32, height as i32);
            gl::ClearColor(0.2, 0.3, 0.3, 1.);
            gl::Clear(gl::COLOR_BUFFER_BIT);

            text_shader.r#use();

            let color_black: [GLfloat; 4] = [0., 0., 0., 1.];
            text_shader.uniform4vf("color", color_black);
            text_shader.uniform1f("scale", camera_scale);
            text_shader.uniform2i("screenSize", [width as i32, height as i32]);
            text_shader.uniform1f("yCenter", gfx_state.center_y);

            // Rendering logic put into separate functions to alleviate nesting
            let x_start = round_to_scale(MARGIN, camera_scale);
            let cursor_coords = render_text(&logic_state, &mut atlas, x_start, 0., &text_shader);

            shape_shader.r#use();
            shape_shader.uniform1f("scale", camera_scale);
            shape_shader.uniform2i("screenSize", [width as i32, height as i32]);
            shape_shader.uniform1f("yCenter", gfx_state.center_y);
            render_cursor(
                &shape_shader,
                cursor_coords,
                atlas.ascender(),
                atlas.descender(),
                cursor_visible,
                &logic_state,
            );
        }
        window.gl_swap_window();
    }

    // Cleanup
    unsafe {
        gl::DeleteBuffers(1, &vbo);
    }
}

fn handle_events_normal(event_pump: &mut EventPump, old_state: &LogicState) -> LogicState {
    use Event::*;
    use Keycode::*;
    let mut state = old_state.clone();
    for event in event_pump.poll_iter() {
        match event {
            Quit { .. } => state.exit = true,
            KeyDown {
                keycode: Some(I), ..
            } => {
                state.mode = EditorMode::Insert;
            }
            _ => {}
        }
    }
    state
}

fn handle_events_insert(
    event_pump: &mut EventPump,
    old_state: &LogicState,
    clipboard: &ClipboardUtil,
) -> LogicState {
    use Event::*;
    use Keycode::*;
    let mut state = old_state.clone();
    for event in event_pump.poll_iter() {
        match event {
            Quit { .. } => state.exit = true,
            KeyDown {
                keycode: Some(Escape),
                ..
            } => state.mode = EditorMode::Normal,
            KeyDown {
                keycode: Some(Left),
                ..
            } => {
                if state.cursor_col == 0 {
                    state.cursor_row = state.cursor_row.saturating_sub(1);
                    let line_len = state
                        .text_buffer
                        .lines()
                        .nth(state.cursor_row)
                        .map(str::len);

                    if let Some(line_len) = line_len {
                        state.cursor_col = line_len;
                    } else {
                        state.cursor_col = 0;
                        state.cursor_row = 0;
                    }
                } else {
                    state.cursor_col = state.cursor_col.saturating_sub(1);
                }
            }
            KeyDown {
                keycode: Some(Backspace),
                ..
            } => {
                let removed_char = state.text_buffer.pop();
                if removed_char.is_some() {
                    state.cursor_col -= 1;
                }

                if removed_char == (Some('\n')) {
                    state.line_count -= 1;
                    // TODO: handle col
                    state.cursor_row -= 1;
                }
            }
            other if other == INSERT_COPY => {
                log_err!(clipboard.set_clipboard_text(&state.text_buffer));
            }
            other if other == INSERT_PASTE => match clipboard.clipboard_text() {
                Ok(t) => {
                    state.push_str(&t);
                }
                Err(e) => eprintln!("{}", e),
            },
            other if other == INSERT_PRINT_FONTS => {
                todo!()
            }
            KeyDown {
                keycode: Some(Return),
                ..
            } => {
                state.text_buffer.push('\n');
                state.line_count += 1;
                state.cursor_col = 0;
                state.cursor_row += 1;
            }
            TextInput { text, .. } => {
                state.push_str(&text);
            }
            _ => {}
        }
    }

    state
}

impl LogicState {
    pub fn push_str(&mut self, s: &str) {
        let new = s.replace('\r', "");
        self.text_buffer.push_str(&new);
        let col_displ = new
            .lines()
            .last()
            .map(|line| line.chars().count())
            .unwrap_or(0);

        let row_displ = new.lines().count().saturating_sub(1);
        if row_displ > 0 {
            self.cursor_col = 0;
        }

        self.cursor_col += col_displ;
        self.cursor_row += row_displ;
    }
}

#[inline]
// TODO: switch to the callback based system
fn check_err() {
    let errors: Vec<_> = iter::repeat(())
        .map_while(|_| {
            let res = unsafe { gl::GetError() };
            if res == gl::NO_ERROR {
                None
            } else {
                Some(res)
            }
        })
        .collect();

    assert!(errors.is_empty(), "Error(s) occurred: {:?}", errors);
}

struct KeyBind {
    key: Keycode,
    modifier: Mod,
}

impl KeyBind {
    pub const CTRL_MOD: Mod = Mod::LCTRLMOD.union(Mod::RCTRLMOD);

    const fn ctrl(key: Keycode) -> Self {
        Self {
            key,
            modifier: Self::CTRL_MOD,
        }
    }
}

impl PartialEq<KeyBind> for Event {
    fn eq(&self, keybind: &KeyBind) -> bool {
        match self {
            Event::KeyDown {
                keycode: Some(key),
                keymod,
                ..
            } => key == &keybind.key && keybind.modifier.contains(*keymod),
            _ => false,
        }
    }
}

#[derive(PartialEq)]
struct GraphicsState {
    camera_scale: f32,
    center_y: f32,
    cursor_visible: bool,
}

#[derive(Clone, PartialEq, Eq)]
struct LogicState {
    exit: bool,
    text_buffer: String,
    line_count: usize,
    cursor_col: usize,
    cursor_row: usize,
    mode: EditorMode,
}

#[derive(Clone, PartialEq, Eq)]
enum EditorMode {
    Insert,
    Normal,
}

struct TimeInterpolator {
    pub start: Instant,
    pub duration: Duration,
    pub start_value: f32,
    pub end_value: f32,
}

impl TimeInterpolator {
    pub fn new(val: f32, duration: Duration) -> Self {
        Self {
            start: Instant::now(),
            duration,
            start_value: val,
            end_value: val,
        }
    }

    pub fn interpolated_value(&self) -> f32 {
        let elapsed_s = self.start.elapsed().as_secs_f32();
        let percent_elapsed = elapsed_s / self.duration.as_secs_f32();
        if percent_elapsed <= 1. {
            (self.end_value - self.start_value).mul_add(percent_elapsed, self.start_value)
        } else {
            self.end_value
        }
    }

    pub fn reset(&mut self, new_value: f32) {
        self.start_value = self.interpolated_value();
        self.end_value = new_value;
        self.start = Instant::now();
    }
}

fn render_cursor(
    shape_shader: &Shader<6>,
    cursor_coords: (f32, f32),
    ascender: f32,
    descender: f32,
    cursor_visible: bool,
    state: &LogicState,
) {
    let (x, y) = cursor_coords;
    let asc = ascender;
    let dsc = descender;
    let alpha = 0.25 + f32::from(u8::from(cursor_visible)) * 0.5;
    let x1 = x;
    let cursor_width = match state.mode {
        EditorMode::Insert => INSERT_CURSOR_WIDTH,
        _ => NORMAL_CURSOR_WIDTH,
    };

    let x2 = x1 + cursor_width;
    let vertices = [
        [x2, y - dsc, 1., 1., 1., alpha],
        [x2, y - asc, 1., 1., 1., alpha],
        [x1, y - asc, 1., 1., 1., alpha],
        [x1, y - dsc, 1., 1., 1., alpha],
    ];

    shape_shader.upload_rectangles(&[vertices]);
    unsafe { gl::DrawElements(gl::TRIANGLES, 6, gl::UNSIGNED_INT, ptr::null()) }
}

// TODO: when building atlas, keep track of width of all characters (and be able
// to predict how wide some text will be)
// also I really need to document this kek
fn render_text(
    state: &LogicState,
    atlas: &mut GlyphAtlas,
    x_start: f32,
    y_start: f32,
    text_shader: &Shader<4>,
) -> (f32, f32) {
    let line_height = atlas.line_height();
    let text = &state.text_buffer;

    atlas.add_characters(text.chars());
    let mut y0 = y_start;
    let mut cursor_coords = (x_start, y_start);

    // Pre-allocate 4 vertices per character. Possibly inexact, but good enough
    let mut vertices_full = Vec::with_capacity(text.len() * 4);
    for (row_idx, line) in text.split('\n').enumerate() {
        let mut x0 = x_start;
        for (col_idx, c) in line.chars().enumerate() {
            let (vertices, ax, ay) = atlas.get_glyph_data(c, x0, y0);
            vertices_full.push(vertices);

            x0 += ax;
            y0 += ay;
            if col_idx + 1 == state.cursor_col {
                cursor_coords.0 = x0;
            }
        }

        if row_idx == state.cursor_row {
            cursor_coords.1 = y0;
        }
        y0 += line_height;
    }

    text_shader.upload_rectangles(&vertices_full);
    check_err();
    unsafe {
        gl::DrawElements(
            gl::TRIANGLES,
            (vertices_full.len() * 6) as i32,
            gl::UNSIGNED_INT,
            ptr::null(),
        );
    }
    check_err();
    cursor_coords
}
