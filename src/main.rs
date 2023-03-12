extern crate sdl2;

use atlas::GlyphAtlas;
use crossfont::{FontDesc, Rasterize, Rasterizer, Size, Slant, Style, Weight};
use gl::types::{GLenum, GLfloat, GLint, GLuint};
use sdl2::event::Event;
use sdl2::keyboard::{Keycode, Mod, Scancode};

use std::ffi::CString;
use std::mem::{self, size_of, size_of_val};
use std::str;
use std::time::{Duration, Instant};
use std::{iter, ptr};

/*  To Do
   To do eventually
   - Change font rendering
   - Add font picker
*/

const INSERT_CURSOR_WIDTH: f32 = 0.25;
const MAX_SCALE: f32 = 64.;
const REDRAW_EVERY: u64 = 1 << 20;
const BLINK_TIME: Duration = Duration::from_millis(500);
const MARGIN: f32 = 1.5;
const SCALE_ANIM_TIME: Duration = Duration::from_millis(100);
const SCROLL_ANIM_TIME: Duration = Duration::from_millis(100);
const CENTER_OFFSET: f32 = -0.5;

mod atlas;
mod rope;
macro_rules! log_err {
    ($e:expr) => {
        let e = $e;
        if let Err(e) = e {
            eprintln!("{}", e);
        }
    };
}

macro_rules! gl_err {
    ($id:path, $iv_fun:path, $info_fun:path, $pname:path) => {
        // Get the compile status
        let mut status = gl::FALSE as GLint;
        $iv_fun($id, $pname, &mut status);

        // Fail on error
        if status != (gl::TRUE as GLint) {
            let mut len = 0;
            $iv_fun($id, gl::INFO_LOG_LENGTH, &mut len);
            let mut buf = vec![0; len as usize];
            $info_fun($id, len, ptr::null_mut(), buf.as_mut_ptr().cast());
            panic!(
                "{}",
                str::from_utf8(&buf).ok().expect("InfoLog not valid utf8")
            );
        }
    };
}

fn compile_shader(src: &str, ty: GLenum) -> u32 {
    let shader;
    unsafe {
        shader = gl::CreateShader(ty);
        // Attempt to compile the shader
        let c_str = CString::new(src.as_bytes()).unwrap();
        gl::ShaderSource(shader, 1, &c_str.as_ptr(), ptr::null());
        gl::CompileShader(shader);
        gl_err!(
            shader,
            gl::GetShaderiv,
            gl::GetShaderInfoLog,
            gl::COMPILE_STATUS
        );
    }
    shader
}

pub fn main() {
    let font_desc = FontDesc::new(
        "vera",
        Style::Description {
            slant: Slant::Normal,
            weight: Weight::Normal,
        },
    );
    let mut rast = Rasterizer::new(1.).expect("Could not set up rasterizer");
    let font_key = rast
        .load_font(&font_desc, Size::new(0.))
        .expect("Could not load font");

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
    let mod_ctrl: Mod = Mod::LCTRLMOD | Mod::RCTRLMOD;
    // TODO: clear up usage of this timer. Can probably be replaced with
    // run_timer
    let mut start = Instant::now();

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
    // TODO: currently bugged. Probably need to disable the attribs before
    // enabling the others
    let shape_shader = Shader::shape_shader(vbo);

    // setup glyph atlas TODO: move this into new
    let mut texture1 = 0;
    unsafe {
        text_shader.r#use();
        gl::GenTextures(1, &mut texture1);
        gl::BindTexture(gl::TEXTURE_2D, texture1);

        // wrapping params
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
        // filtering params
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);

        text_shader.uniform1i("texture1", 0);
    }

    let mut screen_size = window.drawable_size();
    // Buffer for the text edited on screen. Lines are \n terminated
    let mut text_buffer = String::new();
    let mut last_camera_scale = MAX_SCALE;
    let mut atlas = GlyphAtlas::new(rast, font_key, texture1);
    let mut last_recorded_frame = 0;
    let mut scale_animation = TimeInterpolator {
        start,
        start_value: last_camera_scale,
        duration: SCALE_ANIM_TIME,
        end_value: last_camera_scale,
    };

    let mut last_center_y = CENTER_OFFSET;
    let mut scroll_animation = TimeInterpolator {
        start,
        start_value: last_center_y,
        end_value: last_center_y,
        duration: SCROLL_ANIM_TIME,
    };

    let mut _line_count = 0;
    let mut cursor_row = 0;
    let mut cursor_col = 0;

    let mut last_cursor_visible = false;
    let run_timer = Instant::now();

    'running: for frame_counter in 0.. {
        let mut state = UpdateState::new();
        let kbs = event_pump.keyboard_state();
        let ctrl_pressed =
            kbs.is_scancode_pressed(Scancode::LCtrl) | kbs.is_scancode_pressed(Scancode::RCtrl);
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
                    let removed_char = text_buffer.pop();
                    if removed_char.is_some() {
                        state.text = true;
                        cursor_col -= 1;
                    }
                    if removed_char == (Some('\n')) {
                        _line_count -= 1;
                        // TODO: handle col
                        cursor_row -= 1;
                        state.scroll = true;
                    }
                }
                Event::KeyDown {
                    keycode: Some(Keycode::C),
                    keymod,
                    ..
                } if keymod.intersects(mod_ctrl) => {
                    log_err!(clipboard.set_clipboard_text(&text_buffer));
                }
                Event::KeyDown {
                    keycode: Some(Keycode::V),
                    keymod,
                    ..
                } if keymod.intersects(mod_ctrl) => match clipboard.clipboard_text() {
                    Ok(t) => {
                        text_buffer += &t;
                        cursor_col += t.len();
                        state.text = true;
                    }
                    Err(e) => eprintln!("{}", e),
                },
                Event::KeyDown {
                    keycode: Some(Keycode::Return),
                    ..
                } => {
                    text_buffer.push('\n');
                    state.text = true;
                    _line_count += 1;
                    cursor_col = 0;
                    cursor_row += 1;
                    state.scroll = true;
                }
                Event::TextInput { text, .. } if !ctrl_pressed => {
                    // TODO: ensure there are no \r characters whenever we
                    // are inserting text. Also, DRY this up.
                    text_buffer += &text;
                    cursor_col += text.len();
                    state.text = true;
                }
                _ => {}
            }
        }

        // fps tracking
        if start.elapsed().as_secs_f32() >= 0.5 {
            let elapsed_frames = frame_counter - last_recorded_frame;
            let fps = elapsed_frames as f32 / start.elapsed().as_secs_f32();
            window
                .set_title(&format!("Saphedit, fps={fps:.0}"))
                .expect("String has no null bytes");
            last_recorded_frame = frame_counter;
            start = Instant::now();
        }

        // Timed redraw
        state.timed = frame_counter % REDRAW_EVERY == 0;

        // Update screen size
        let new_screen_size = window.drawable_size();
        state.resize = new_screen_size != screen_size;
        screen_size = new_screen_size;

        // Update text size / update scale
        if state.text || state.resize {
            let (text_w, text_h) = atlas.measure_dims(text_buffer.chars());
            let scale_x = new_screen_size.0 as f32 / (text_w + 2. * MARGIN);
            let scale_y = new_screen_size.1 as f32 / text_h;
            // TODO: do a better estimate of the size; the issue here is that
            // the theoretical scale depends on the text size, which can change
            // from one scale to another
            let new_scale_raw = scale_x.min(scale_y).max(8.).min(MAX_SCALE);
            let step = GlyphAtlas::SCALE_STEP;
            let new_scale_rounded = (new_scale_raw / step).floor() * step;
            scale_animation.reset(new_scale_rounded);
        }

        let camera_scale = scale_animation.interpolated_value();
        state.rescale |= camera_scale != last_camera_scale;
        last_camera_scale = camera_scale;

        atlas.select_scale(camera_scale);

        // Cursor update
        let time_period = (run_timer.elapsed().as_secs_f32() / BLINK_TIME.as_secs_f32()) as u32;
        let cursor_visible = time_period % 2 == 0;
        state.blink_change = cursor_visible ^ last_cursor_visible;
        last_cursor_visible = cursor_visible;

        // Scroll update
        if state.scroll {
            let y_center_new_target =
                cursor_row as f32 * atlas.line_height() as f32 + CENTER_OFFSET;
            scroll_animation.reset(y_center_new_target);
        }
        let y_center_new = scroll_animation.interpolated_value();
        state.animating |= y_center_new != last_center_y;
        last_center_y = y_center_new;

        // Dear Princess Celestia
        // I fucking hate indentation
        // Your faithful student
        // Twinkle Springle
        if !state.needs_redraw() {
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
            text_shader.uniform1f("yCenter", last_center_y);

            // Rendering logic put into separate functions to alleviate nesting
            let cursor_coords = render_text(
                &text_buffer,
                &mut atlas,
                MARGIN,
                0.,
                cursor_row,
                cursor_col,
                &text_shader,
            );

            shape_shader.r#use();
            shape_shader.uniform1f("scale", camera_scale);
            shape_shader.uniform2i("screenSize", [width as i32, height as i32]);
            shape_shader.uniform1f("yCenter", last_center_y);
            render_cursor(
                &shape_shader,
                cursor_coords,
                atlas.ascender(),
                atlas.descender(),
                cursor_visible,
            );
        }
        window.gl_swap_window();
    }

    // Cleanup
    unsafe {
        gl::DeleteBuffers(1, &vbo);
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

struct TimeInterpolator {
    pub start: Instant,
    pub duration: Duration,
    pub start_value: f32,
    pub end_value: f32,
}

impl TimeInterpolator {
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
) {
    let (x, y) = cursor_coords;
    let asc = ascender;
    let dsc = descender;
    let alpha = 0.25 + f32::from(u8::from(cursor_visible)) * 0.5;
    let x1 = x;
    let x2 = x1 + INSERT_CURSOR_WIDTH;
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
    text: &str,
    atlas: &mut GlyphAtlas,
    x_start: f32,
    y_start: f32,
    cursor_row: usize,
    cursor_col: usize,
    text_shader: &Shader<4>,
) -> (f32, f32) {
    let line_height = atlas.line_height();

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
            if col_idx + 1 == cursor_col {
                cursor_coords.0 = x0;
            }
        }

        if row_idx == cursor_row {
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

#[repr(C, packed)]
#[derive(Debug)]
struct UpdateState {
    text: bool,
    resize: bool,
    rescale: bool,
    timed: bool,
    animating: bool,
    blink_change: bool,
    scroll: bool,
}

impl UpdateState {
    fn new() -> Self {
        // safety: 0 is a valid value for all booleans
        unsafe { mem::zeroed() }
    }

    fn needs_redraw(self) -> bool {
        // horrible fucking hack
        // safety: struct is packed, so each boolean gets its own byte
        let as_array: [bool; size_of::<Self>()] = unsafe { mem::transmute(self) };

        as_array
            .iter()
            .copied()
            .reduce(|a, b| a | b)
            .expect("Array can't be empty")
    }
}

struct Shader<const N: usize> {
    program_id: GLuint,
    vao: GLuint,
    ebo: GLuint,
}

struct AttributeInfo<'a> {
    size: u32,
    name: &'a str,
}

const TEXT_SHADER_ATTR_INFO: [AttributeInfo; 2] = [
    AttributeInfo {
        size: 2,
        name: "aPos",
    },
    AttributeInfo {
        size: 2,
        name: "aTexCoord",
    },
];

const SHAPE_SHADER_ATTR_INFO: [AttributeInfo; 2] = [
    AttributeInfo {
        size: 2,
        name: "aPos",
    },
    AttributeInfo {
        size: 4,
        name: "inColour",
    },
];

impl<const N: usize> Shader<N> {
    /// Creates a new shader in `SHADER_PATH/{shader_name}_*.glsl`
    /// ### Safety
    /// Caller must ensure that the attribute info is valid for the shader
    // TODO: make this safe (should be easy)
    unsafe fn new(vbo: GLuint, vs_src: &str, fs_src: &str, attr_info: &[AttributeInfo]) -> Self {
        let vertex_shader_id = compile_shader(vs_src, gl::VERTEX_SHADER);
        let fragment_shader_id = compile_shader(fs_src, gl::FRAGMENT_SHADER);
        let program_id = {
            let shader_program = gl::CreateProgram();
            gl::AttachShader(shader_program, vertex_shader_id);
            gl::AttachShader(shader_program, fragment_shader_id);
            gl::LinkProgram(shader_program);
            gl::DeleteShader(vertex_shader_id);
            gl::DeleteShader(fragment_shader_id);
            gl_err!(
                shader_program,
                gl::GetProgramiv,
                gl::GetProgramInfoLog,
                gl::LINK_STATUS
            );
            shader_program
        };

        gl::UseProgram(program_id);

        let mut vao = 0;
        let mut ebo = 0;

        gl::GenVertexArrays(1, &mut vao);
        gl::GenBuffers(1, &mut ebo);

        gl::BindVertexArray(vao);
        gl::BindBuffer(gl::ARRAY_BUFFER, vbo);

        gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, ebo);

        let stride: u32 = attr_info
            .iter()
            .map(|attr| attr.size * mem::size_of::<GLfloat>() as u32)
            .sum();

        let mut offset = 0;
        for AttributeInfo {
            size,
            name: attr_name,
        } in attr_info
        {
            let name = c_str(attr_name);
            let attr_location = gl::GetAttribLocation(program_id, name.as_ptr());
            assert!(attr_location >= 0, "Couldn't find attribute {attr_name}");

            let attr_location = attr_location.try_into().unwrap();
            let pointer = ptr::null::<GLfloat>().wrapping_add(offset);

            gl::VertexAttribPointer(
                attr_location,
                *size as i32,
                gl::FLOAT,
                gl::FALSE,
                stride as i32,
                pointer.cast(),
            );

            check_err();

            offset += *size as usize;

            gl::EnableVertexAttribArray(attr_location);
        }
        Self {
            program_id,
            vao,
            ebo,
        }
    }

    /// Uploads the rectangles present in data to the GPU. The innermost array
    /// is the vertex data uploaded to each vertex. Each entry of the outermost
    /// array has 4 sub-entries, in this order:
    /// * top right
    /// * bottom right
    /// * bottom left
    /// * top left
    fn upload_rectangles(&self, data: &[[[f32; N]; 4]]) {
        if data.is_empty() {
            return;
        }

        let indices = (0..data.len())
            .flat_map(|n| {
                let offset = n as GLuint * 4;

                #[allow(clippy::identity_op)] // helps readability
                [
                    [offset + 1, offset + 2, offset + 3],
                    [offset + 0, offset + 1, offset + 3],
                ]
            })
            .collect::<Vec<_>>();

        unsafe {
            gl::BindVertexArray(self.vao);
            let elem_array_size = size_of_val(indices.as_slice());
            gl::BufferData(
                gl::ELEMENT_ARRAY_BUFFER,
                elem_array_size as isize,
                indices.as_ptr().cast(),
                gl::DYNAMIC_DRAW,
            );

            // Safety:
            // - vbo initialised and bound
            gl::BufferData(
                gl::ARRAY_BUFFER,
                size_of_val(data) as isize,
                data.as_ptr().cast(),
                gl::DYNAMIC_DRAW,
            );
        }
    }

    fn r#use(&self) {
        unsafe {
            gl::UseProgram(self.program_id);
        }
    }

    fn uniform1i(&self, name: &str, val: i32) {
        let name = c_str(name);
        unsafe {
            gl::Uniform1i(gl::GetUniformLocation(self.program_id, name.as_ptr()), val);
        }
    }

    fn uniform1f(&self, name: &str, val: GLfloat) {
        let name = c_str(name);
        unsafe {
            gl::Uniform1f(gl::GetUniformLocation(self.program_id, name.as_ptr()), val);
        }
    }

    fn uniform4vf(&self, name: &str, val: [GLfloat; 4]) {
        let name = c_str(name);
        unsafe {
            gl::Uniform4fv(
                gl::GetUniformLocation(self.program_id, name.as_ptr()),
                1,
                val.as_ptr(),
            );
        }
    }

    fn uniform2i(&self, name: &str, val: [GLint; 2]) {
        let name = c_str(name);
        unsafe {
            gl::Uniform2i(
                gl::GetUniformLocation(self.program_id, name.as_ptr()),
                val[0],
                val[1],
            );
        }
    }
}

impl Shader<4> {
    fn text_shader(vbo: GLuint) -> Self {
        unsafe {
            // Safety: the sizes in TEXT_SHADER_ATTR_INFO sum up to 4
            Self::new(
                vbo,
                include_str!("shaders/text_vertex.glsl"),
                include_str!("shaders/text_fragment.glsl"),
                &TEXT_SHADER_ATTR_INFO,
            )
        }
    }
}

impl Shader<6> {
    fn shape_shader(vbo: GLuint) -> Self {
        unsafe {
            Self::new(
                vbo,
                include_str!("shaders/shape_vertex.glsl"),
                include_str!("shaders/shape_fragment.glsl"),
                &SHAPE_SHADER_ATTR_INFO,
            )
        }
    }
}

fn c_str(name: &str) -> CString {
    CString::new(name).expect("Name needs to be valid ascii")
}

impl<const N: usize> Drop for Shader<N> {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteProgram(self.program_id);
            gl::DeleteVertexArrays(1, &self.vao);
            gl::DeleteBuffers(1, &self.ebo);
        };
    }
}
