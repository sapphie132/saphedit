extern crate sdl2;

use atlas::GlyphAtlas;
use crossfont::{FontDesc, Rasterize, Rasterizer, Size, Slant, Style, Weight};
use gl::types::{GLchar, GLenum, GLfloat, GLint, GLuint};
use sdl2::event::Event;
use sdl2::keyboard::{Keycode, Mod, Scancode};

use std::ffi::CString;
use std::fs::read_to_string;
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
            $iv_fun($id, $pname, &mut len);
            let mut buf = Vec::with_capacity(len as usize);
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

const VS_SRC_PATH: &str = "src/shaders/vertex.glsl";
const FS_SRC_PATH: &str = "src/shaders/fragment.glsl";

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

    let window = video_subsystem
        .window("Saphedit", 800, 600)
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
    let mut frame_counter = 0;
    let mut start = Instant::now();

    let mut vbo = 0;
    let mut vao = 0;
    let mut ebo = 0;

    let shader = Shader::new();

    // let img = image::open("A.png").unwrap().to_rgba8();

    // setup program
    unsafe {
        // Safe code, but the variables aren't needed outside this block

        let indices = [
            1, 2, 3, // Second triangle
            0, 1, 3, // first triangle
        ];
        gl::GenVertexArrays(1, &mut vao);
        gl::GenBuffers(1, &mut vbo);
        gl::GenBuffers(1, &mut ebo);

        gl::BindVertexArray(vao);

        gl::BindBuffer(gl::ARRAY_BUFFER, vbo);

        gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, ebo);
        gl::BufferData(
            gl::ELEMENT_ARRAY_BUFFER,
            size_of_val(&indices) as isize,
            mem::transmute(&indices),
            gl::STATIC_DRAW,
        );

        // position attribute
        gl::VertexAttribPointer(
            0,
            2,
            gl::FLOAT,
            gl::FALSE,
            4 * size_of::<GLfloat>() as i32,
            ptr::null(),
        );
        gl::EnableVertexAttribArray(0);

        // coordinate attribute
        gl::VertexAttribPointer(
            1,
            2,
            gl::FLOAT,
            gl::FALSE,
            4 * size_of::<GLfloat>() as i32,
            mem::transmute(2 * size_of::<GLfloat>()),
        );
        gl::EnableVertexAttribArray(1);

        gl::Enable(gl::BLEND);
        gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
    };

    // setup glyph atlas TODO: move this into new
    let mut texture1 = 0;
    unsafe {
        gl::GenTextures(1, &mut texture1);
        gl::BindTexture(gl::TEXTURE_2D, texture1);

        // wrapping params
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
        // filtering params
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);

        shader.r#use();
        shader.uniform1i("texture1", 0);
    }

    let mut screen_size = window.drawable_size();
    let mut text_buffer = String::new();
    let mut camera_scale = 128;
    let mut atlas = GlyphAtlas::new(&mut rast, font_key, texture1, camera_scale).unwrap();
    'running: loop {
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
        if start.elapsed().as_secs() >= 2 {
            let fps = frame_counter as f64 / start.elapsed().as_secs_f64();
            println!("running at {:.2} fps", fps);
            frame_counter = 0;
            start = Instant::now();
        }
        frame_counter += 1;

        // Update screen size
        let new_screen_size = window.drawable_size();
        state.resize = new_screen_size != screen_size;
        screen_size = new_screen_size;

        // Update text size
        if state.text {
            let old_scale = camera_scale;
            let (text_w, text_h) = atlas.measure_dims(text_buffer.chars());
            let scale_x = new_screen_size.0 as f32 / text_w;
            let scale_y = new_screen_size.1 as f32 / text_h;
            let new_scale = scale_x.min(scale_y).max(8.).min(1024.);
            camera_scale = new_scale.floor() as u32;
            state.rescale |= camera_scale != old_scale;
        }

        if state.rescale {
            atlas = GlyphAtlas::new(&mut rast, font_key, texture1, camera_scale).unwrap();
        }

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

            shader.r#use();

            let color_black: [GLfloat; 4] = [0., 0., 0., 1.];
            shader.uniform4vf("color", color_black);
            shader.uniform1f("scale", camera_scale as f32);
            shader.uniform2i("screenSize", [width as i32, height as i32]);

            // rast.update_dpr(camera_scale); TODO: add me back (somewher)
            render_text(&text_buffer, &mut atlas, 0., 0., texture1, vao, &mut rast);
        }
        window.gl_swap_window();
    }

    // Cleanup
    unsafe {
        gl::DeleteVertexArrays(1, &vao);
        gl::DeleteBuffers(1, &vbo);
        gl::DeleteBuffers(1, &ebo);
    }
}

// TODO: when building atlas, keep track of width of all characters (and be able
// to predict how wide some text will be)
// also I really need to document this kek
fn render_text(
    text: &str,
    atlas: &mut GlyphAtlas,
    x_start: f32,
    y_start: f32,
    texture1: GLuint,
    vao: GLuint,
    rast: &mut Rasterizer,
) {
    let letter_height = 64.;
    // TODO: adjust for scale
    let line_height = letter_height as f32 * Size::factor();

    atlas.add_characters(text.chars(), texture1, rast);
    let mut y0 = y_start;
    for line in text.lines() {
        let mut x0 = x_start;
        for c in line.chars() {
            let (vertices, ax, ay) = atlas.get_glyph_data(c, x0, y0);
            unsafe {
                gl::BindVertexArray(vao);
                gl::BufferData(
                    gl::ARRAY_BUFFER,
                    size_of_val(&vertices) as isize,
                    mem::transmute(&vertices),
                    gl::STATIC_DRAW,
                );

                // gl::GenerateMipmap(gl::TEXTURE_2D);
                gl::DrawElements(gl::TRIANGLES, 6, gl::UNSIGNED_INT, ptr::null());
            }

            x0 += ax as f32;
            y0 += ay as f32;
        }
        y0 += line_height as f32;
    }
}

#[repr(C, packed)]
struct UpdateState {
    text: bool,
    resize: bool,
    rescale: bool,
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

struct Shader(GLuint);

impl Shader {
    fn new() -> Self {
        let vs_source = read_to_string(VS_SRC_PATH).expect("Could not read vertex shader source");
        let fs_source = read_to_string(FS_SRC_PATH).expect("Could not read vertex shader source");
        let vertex_shader_id = compile_shader(&vs_source, gl::VERTEX_SHADER);
        let fragment_shader_id = compile_shader(&fs_source, gl::FRAGMENT_SHADER);
        let shader = unsafe {
            let shader_program = gl::CreateProgram();
            gl::AttachShader(shader_program, vertex_shader_id);
            gl::AttachShader(shader_program, fragment_shader_id);
            gl::LinkProgram(shader_program);
            // gl::DeleteShader(vertex_shader_id);
            // gl::DeleteShader(fragment_shader_id);
            gl_err!(
                shader_program,
                gl::GetProgramiv,
                gl::GetProgramInfoLog,
                gl::LINK_STATUS
            );
            shader_program
        };
        Self(shader)
    }

    fn r#use(&self) {
        unsafe {
            gl::UseProgram(self.0);
        }
    }

    fn uniform1i(&self, name: &str, val: i32) {
        let name = c_str(name);
        unsafe {
            gl::Uniform1i(gl::GetUniformLocation(self.0, name.as_ptr()), val);
        }
    }

    fn uniform1f(&self, name: &str, val: GLfloat) {
        let name = c_str(name);
        unsafe {
            gl::Uniform1f(gl::GetUniformLocation(self.0, name.as_ptr()), val);
        }
    }

    fn uniform4vf(&self, name: &str, val: [GLfloat; 4]) {
        let name = c_str(name);
        unsafe {
            gl::Uniform4fv(
                gl::GetUniformLocation(self.0, name.as_ptr()),
                1,
                val.as_ptr(),
            );
        }
    }

    fn uniform2i(&self, name: &str, val: [GLint; 2]) {
        let name = c_str(name);
        unsafe {
            gl::Uniform2i(
                gl::GetUniformLocation(self.0, name.as_ptr()),
                val[0],
                val[1],
            )
        }
    }
}

fn c_str(name: &str) -> CString {
    CString::new(name).expect("Name needs to be valid ascii")
}

impl Drop for Shader {
    fn drop(&mut self) {
        unsafe { gl::DeleteProgram(self.0) };
    }
}
