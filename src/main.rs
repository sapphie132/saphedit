extern crate sdl2;

use atlas::GlyphAtlas;
use crossfont::{FontDesc, Rasterize, Rasterizer, Size, Slant, Style, Weight};
use gl::types::{GLchar, GLenum, GLfloat, GLint, GLuint};
use sdl2::event::Event;
use sdl2::keyboard::{Keycode, Mod, Scancode};

use std::ffi::CString;
use std::mem::{self, size_of, size_of_val};
use std::ptr;
use std::str;
use std::time::Instant;

/*  To Do
   To do eventually
   - Change font rendering
   - Add font picker
*/

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
            $info_fun($id, len, ptr::null_mut(), buf.as_mut_ptr() as *mut GLchar);
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

const REDRAW_EVERY: u64 = 1 << 30;

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
    gl::load_with(|name| video_subsystem.gl_get_proc_address(name) as *const _);

    let mut event_pump = sdl_context.event_pump().unwrap();
    let mod_ctrl: Mod = Mod::LCTRLMOD | Mod::RCTRLMOD;
    let mut start = Instant::now();

    // setup blending
    unsafe {
        gl::Enable(gl::BLEND);
        gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
    };

    let text_shader = Shader::text_shader();
    // TODO: currently bugged. Probably need to disable the attribs before
    // enabling the others
    // let shape_shader = Shader::shape_shader();

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
    let mut text_buffer = String::new();
    let mut last_camera_scale = 1.;
    let mut atlas = GlyphAtlas::new(rast, font_key, texture1);
    let mut last_recorded_frame = 0;
    let mut scale_animation = ScaleAnimation {
        start,
        start_value: last_camera_scale as f32,
        end_value: last_camera_scale as f32,
    };
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
                    state.text |= text_buffer.pop().is_some();
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
                        state.text = true;
                    }
                    Err(e) => eprintln!("{}", e),
                },
                Event::KeyDown {
                    keycode: Some(Keycode::Return),
                    ..
                } => {
                    text_buffer.push_str("\n");
                    state.text = true;
                }
                Event::TextInput { text, .. } if !ctrl_pressed => {
                    text_buffer += &text;
                    state.text = true;
                }
                _ => {}
            }
        }

        // fps tracking
        if start.elapsed().as_secs_f32() >= 0.5 {
            let elapsed_frames = frame_counter - last_recorded_frame;
            let fps = elapsed_frames as f64 / start.elapsed().as_secs_f64();
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

        // Update text size
        if state.text || state.resize {
            let (text_w, text_h) = atlas.measure_dims(text_buffer.chars());
            let scale_x = new_screen_size.0 as f64 / text_w;
            let scale_y = new_screen_size.1 as f64 / text_h;
            // Empirical maximum size. It should be possible to get an actual maximum size
            // (TODO)
            let new_scale = scale_x.min(scale_y).max(8.).min(64.);
            scale_animation = ScaleAnimation {
                start_value: scale_animation.actual_scale(),
                end_value: new_scale as f32,
                start: Instant::now(),
            };
        }

        let camera_scale = scale_animation.actual_scale();
        state.rescale |= camera_scale != last_camera_scale;
        last_camera_scale = camera_scale;

        atlas.select_scale(camera_scale);

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

            render_text(&text_buffer, &mut atlas, 0., 0., &text_shader);
        }
        window.gl_swap_window();
    }
}

struct ScaleAnimation {
    pub start: Instant,
    pub start_value: f32,
    pub end_value: f32,
}

impl ScaleAnimation {
    const ANIM_TIME_S: f32 = 0.2;
    pub fn actual_scale(&self) -> f32 {
        let elapsed_s = self.start.elapsed().as_secs_f32();
        let percent_elapsed = elapsed_s / Self::ANIM_TIME_S;
        if percent_elapsed <= 1. {
            (self.end_value - self.start_value) * percent_elapsed + self.start_value
        } else {
            self.end_value
        }
    }
}

// TODO: when building atlas, keep track of width of all characters (and be able
// to predict how wide some text will be)
// also I really need to document this kek
fn render_text(
    text: &str,
    atlas: &mut GlyphAtlas,
    x_start: f64,
    y_start: f64,
    text_shader: &Shader<4>,
) {
    let line_height = atlas.line_height();

    atlas.add_characters(text.chars());
    let mut y0 = y_start;
    for line in text.lines() {
        let mut x0 = x_start;
        for c in line.chars() {
            let (vertices, ax, ay) = atlas.get_glyph_data(c, x0, y0);
            text_shader.buffer_data(&vertices);
            unsafe {
                // gl::GenerateMipmap(gl::TEXTURE_2D);
                gl::BindVertexArray(text_shader.vao);
                gl::DrawElements(gl::TRIANGLES, 6, gl::UNSIGNED_INT, ptr::null());
                // gl::DrawArrays(gl::TRIANGLES, 0, 1);
            }

            x0 += ax;
            y0 += ay;
        }
        y0 += line_height;
    }
}

#[repr(C, packed)]
struct UpdateState {
    text: bool,
    resize: bool,
    rescale: bool,
    timed: bool,
    animating: bool,
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
    vbo: GLuint,
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
    unsafe fn new(vs_src: &str, fs_src: &str, attr_info: &[AttributeInfo]) -> Self {
        let vertex_shader_id = compile_shader(&vs_src, gl::VERTEX_SHADER);
        let fragment_shader_id = compile_shader(&fs_src, gl::FRAGMENT_SHADER);
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
        let mut vbo = 0;
        let mut ebo = 0;

        gl::GenVertexArrays(1, &mut vao);
        gl::GenBuffers(1, &mut vbo);
        gl::GenBuffers(1, &mut ebo);

        gl::BindVertexArray(vao);
        gl::BindBuffer(gl::ARRAY_BUFFER, vbo);

        gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, ebo);
        let indices = [
            1, 2, 3, // Second triangle
            0, 1, 3, // first triangle
        ];

        gl::BufferData(
            gl::ELEMENT_ARRAY_BUFFER,
            size_of_val(&indices) as isize,
            mem::transmute(&indices),
            gl::DYNAMIC_DRAW,
        );

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
            let attrib_location = gl::GetAttribLocation(program_id, name.as_ptr());
            if attrib_location < 0 {
                panic!("Couldn't find attribute {attr_name}");
            }

            let attrib_location = attrib_location as u32;
            let pointer = ptr::null::<GLfloat>().wrapping_add(offset);

            gl::VertexAttribPointer(
                attrib_location,
                *size as i32,
                gl::FLOAT,
                gl::FALSE,
                stride as i32,
                pointer as _,
            );

            let err = gl::GetError();
            if err != gl::NO_ERROR {
                panic!("Error occurred {err}") // TODO? better error handling?
            }
            offset += *size as usize;

            gl::EnableVertexAttribArray(attrib_location);
        }
        Self {
            program_id,
            vao,
            ebo,
            vbo,
        }
    }

    fn buffer_data(&self, data: &[[f32; N]]) {
        // TODO: make this work for longer arrays
        assert!(
            data.len() == 4,
            "data needs to have an even number of triangles"
        );
        unsafe {
            gl::BindVertexArray(self.vao);
            // Safety:
            // - vbo initialised and bound
            gl::BufferData(
                gl::ARRAY_BUFFER,
                size_of_val(data) as isize,
                data.as_ptr() as _,
                gl::DYNAMIC_DRAW,
            )
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
            )
        }
    }
}

impl Shader<4> {
    fn text_shader() -> Shader<4> {
        unsafe {
            // Safety: the sizes in TEXT_SHADER_ATTR_INFO sum up to 4
            Shader::new(
                include_str!("shaders/text_vertex.glsl"),
                include_str!("shaders/text_fragment.glsl"),
                &TEXT_SHADER_ATTR_INFO,
            )
        }
    }
}

impl Shader<6> {
    fn shape_shader() -> Self {
        unsafe {
            Shader::new(
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
            gl::DeleteBuffers(1, &self.vbo);
            gl::DeleteBuffers(1, &self.ebo);
        };
    }
}
