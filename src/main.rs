extern crate sdl2;

use sdl2::event::Event;
use sdl2::keyboard::{Keycode, Mod};
use sdl2::pixels::Color;
use sdl2::rect::Rect;
use sdl2::render::{Texture, TextureCreator, TextureQuery};

use sdl2::ttf::{self, Font};
use std::time::Instant;

/*  To Do
   - Fix text position and size

   To do eventually
   - Change font rendering
   - Add font picker
*/
macro_rules! log_err {
    ($e:expr) => {
        let e = $e;
        if let Err(e) = e {
            eprintln!("{}", e);
        }
    };
}

const DEFAULT_FONT: &str = "fonts/bitstream-vera-sans-mono-fonts/VeraMono.ttf";

pub fn main() {
    let font = DEFAULT_FONT;
    let sdl_context = sdl2::init().unwrap();
    let video_subsystem = sdl_context.video().unwrap();
    video_subsystem.text_input().start();

    let clipboard = video_subsystem.clipboard();

    let ttf_context = ttf::init().unwrap();
    let font = ttf_context.load_font(&font, 25).unwrap();

    let window = video_subsystem
        .window("Saphedit", 800, 600)
        .position_centered()
        .resizable()
        .build()
        .unwrap();

    let mut canvas = window.into_canvas().build().unwrap();
    let texture_creator = canvas.texture_creator();

    canvas.set_draw_color(Color::RGB(0, 0, 0));
    canvas.clear();
    canvas.present();
    let mut event_pump = sdl_context.event_pump().unwrap();
    let mod_ctrl: Mod = Mod::LCTRLMOD | Mod::RCTRLMOD;
    let mut frame_counter = 0;
    let mut start = Instant::now();

    let mut state = TextState {
        text_buffer: String::new(),
        update_text: true,
    };

    let text_render_info = TextRenderInfo {
        font,
        texture_creator,
    };

    let mut texture =
        make_text_texture(&state, &text_render_info, canvas.window().drawable_size().0);
    'running: loop {
        canvas.clear();
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => break 'running,
                Event::KeyDown {
                    keycode: Some(Keycode::Backspace),
                    ..
                } => {
                    state.update_text |= state.text_buffer.pop().is_some();
                }
                Event::KeyDown {
                    keycode: Some(Keycode::C),
                    keymod,
                    ..
                } if keymod.intersects(mod_ctrl) => {
                    log_err!(clipboard.set_clipboard_text(&state.text_buffer));
                }
                Event::KeyDown {
                    keycode: Some(Keycode::V),
                    keymod,
                    ..
                } if keymod.intersects(mod_ctrl) => match clipboard.clipboard_text() {
                    Ok(t) => {
                        state.text_buffer += &t;
                        state.update_text = true;
                    }
                    Err(e) => eprintln!("{}", e),
                },
                Event::KeyDown {
                    keycode: Some(Keycode::Return),
                    ..
                } => {
                    state.text_buffer.push_str("\n");
                    state.update_text = true;
                }
                Event::TextInput { text, .. } => {
                    state.text_buffer += &text;
                    state.update_text = true;
                }
                _ => {}
            }
        }

        if state.update_text {
            texture =
                make_text_texture(&state, &text_render_info, canvas.window().drawable_size().0);
            state.update_text = false;
        }

        if frame_counter == 512 {
            let fps = frame_counter as f64 / start.elapsed().as_secs_f64();
            println!("{}", fps);
            frame_counter = 0;
            start = Instant::now();
        }

        let target = {
            let TextureQuery { width, height, .. } = texture.query();
            let (scrn_height, scrn_width) = canvas.window().drawable_size();
            get_centered_rect(width, height, scrn_width, scrn_height)
        };
        log_err!(canvas.copy(&texture, None, Some(target)));
        canvas.present();
        frame_counter += 1;
    }
}

struct TextState {
    text_buffer: String,
    update_text: bool,
}

struct TextRenderInfo<'a, 'b, T> {
    font: Font<'a, 'b>,
    texture_creator: TextureCreator<T>,
}

fn make_text_texture<'render, T>(
    state: &TextState,
    text_render_info: &'render TextRenderInfo<T>,
    max_width: u32,
) -> Texture<'render> {
    let to_render = if state.text_buffer.is_empty() {
        " "
    } else {
        &state.text_buffer
    };
    let surface = text_render_info
        .font
        .render(to_render)
        .blended_wrapped(Color::RGBA(255, 0, 0, 255), max_width)
        .unwrap();

    return text_render_info
        .texture_creator
        .create_texture_from_surface(&surface)
        .unwrap();
}

// handle the annoying Rect i32
macro_rules! rect(
    ($x:expr, $y:expr, $w:expr, $h:expr) => (
        Rect::new($x as i32, $y as i32, $w as u32, $h as u32)
    )
);

fn get_centered_rect(rect_width: u32, rect_height: u32, cons_width: u32, cons_height: u32) -> Rect {
    let wr = rect_width as f32 / cons_width as f32;
    let hr = rect_height as f32 / cons_height as f32;

    let (w, h) = if wr > 1f32 || hr > 1f32 {
        if wr > hr {
            println!("Scaling down! The text will look worse!");
            let h = (rect_height as f32 / wr) as i32;
            (cons_width as i32, h)
        } else {
            println!("Scaling down! The text will look worse!");
            let w = (rect_width as f32 / hr) as i32;
            (w, cons_height as i32)
        }
    } else {
        (rect_width as i32, rect_height as i32)
    };

    let cx = (cons_width as i32 - w) / 2;
    let cy = (cons_height as i32 - h) / 2;
    rect!(cx, cy, w, h)
}
