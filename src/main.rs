extern crate sdl2;

use crossfont::{
    BitmapBuffer, FontDesc, FontKey, GlyphKey, Rasterize, Rasterizer, Size, Slant, Style, Weight,
};
use gl::types::{GLchar, GLenum, GLfloat, GLint, GLuint};
use sdl2::event::Event;
use sdl2::keyboard::{Keycode, Mod};

use image::{self, DynamicImage};
use std::ffi::CString;
use std::fs::read_to_string;
use std::mem::{self, size_of, size_of_val};
use std::ptr;
use std::str;
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
            buf.set_len((len as usize) - 1); // subtract 1 to skip the trailing null character
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
    let font_path = DEFAULT_FONT;
    let font_desc = FontDesc::new(
        "vera",
        Style::Description {
            slant: Slant::Normal,
            weight: Weight::Normal,
        },
    );
    let mut rast = Rasterizer::new(1.).expect("Could not set up rasterizer");
    let font_key = rast
        .load_font(&font_desc, Size::new(64.))
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

    let ctx = window.gl_create_context().unwrap();
    gl::load_with(|name| video_subsystem.gl_get_proc_address(name) as *const _);

    let mut event_pump = sdl_context.event_pump().unwrap();
    let mod_ctrl: Mod = Mod::LCTRLMOD | Mod::RCTRLMOD;
    let mut frame_counter = 0;
    let mut start = Instant::now();

    let mut state = TextState {
        text_buffer: String::new(),
        update_text: true,
    };

    let mut vbo = 0;
    let mut vao = 0;
    let mut ebo = 0;

    let shader = Shader::new();

    // let img = image::open("A.png").unwrap().to_rgba8();
    let glyph_key = GlyphKey {
        character: 'A',
        font_key,
        size: Size::new(150.),
    };
    let glyph = rast.get_glyph(glyph_key).unwrap();
    // setup program
    unsafe {
        let top = glyph.top as f32;
        let left = glyph.left as f32;
        let width = glyph.width as f32;
        let height = glyph.height as f32;

        let (win_width, win_height) = window.drawable_size();
        let sx = 1. / win_width as f32;
        let sy = 1. / win_height as f32;

        let x1 = left * sx;
        let w = width * sx;
        let x2 = x1 + w;

        let y1 = top * sx;
        let h = height * sy;
        let y2 = y1 + h;
        // Safe code, but the variables aren't needed outside this block
        let vertices: [GLfloat; 32] = [
            //positions      // colours     // texture coordinates
            x2, y2, 0.0, 1.0, 0.0, 0.0, 1., 0., // top right
            x2, y1, 0.0, 0.0, 1.0, 0.0, 1., 1., // bottom right
            x1, y1, 0.0, 0.0, 0.0, 1.0, 0., 1., // bottom left
            x1, y2, 0.0, 1.0, 1.0, 0.0, 0., 0., // top left
        ];

        let indices = [
            1, 2, 3, // Second triangle
            0, 1, 3, // first triangle
        ];
        gl::GenVertexArrays(1, &mut vao);
        gl::GenBuffers(1, &mut vbo);
        gl::GenBuffers(1, &mut ebo);

        gl::BindVertexArray(vao);

        gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
        gl::BufferData(
            gl::ARRAY_BUFFER,
            size_of_val(&vertices) as isize,
            mem::transmute(&vertices),
            gl::STATIC_DRAW,
        );

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
            3,
            gl::FLOAT,
            gl::FALSE,
            8 * size_of::<GLfloat>() as i32,
            ptr::null(),
        );
        gl::EnableVertexAttribArray(0);

        // position attribute
        gl::VertexAttribPointer(
            1,
            3,
            gl::FLOAT,
            gl::FALSE,
            8 * size_of::<GLfloat>() as i32,
            mem::transmute(3 * size_of::<GLfloat>()),
        );
        gl::EnableVertexAttribArray(1);

        // position attribute
        gl::VertexAttribPointer(
            2,
            2,
            gl::FLOAT,
            gl::FALSE,
            8 * size_of::<GLfloat>() as i32,
            mem::transmute(6 * size_of::<GLfloat>()),
        );
        gl::EnableVertexAttribArray(2);
    };

    // setup glyph atlas
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

        let (pixels, fmt) = get_vec_format(glyph.buffer);

        gl::TexImage2D(
            gl::TEXTURE_2D,
            0,
            gl::RGBA as i32,
            glyph.width as i32,
            glyph.height as i32,
            0,
            fmt,
            gl::UNSIGNED_BYTE,
            pixels.as_ptr() as *const _,
        );
        gl::GenerateMipmap(gl::TEXTURE_2D);

        shader.r#use();
        shader.set_int("texture1", 0);
    }

    'running: loop {
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
            state.update_text = false;
        }

        if frame_counter == 512 {
            let fps = frame_counter as f64 / start.elapsed().as_secs_f64();
            println!("{}", fps);
            frame_counter = 0;
            start = Instant::now();
        }

        frame_counter += 1;

        // draw
        unsafe {
            gl::ClearColor(0.2, 0.3, 0.3, 1.);
            gl::Clear(gl::COLOR_BUFFER_BIT);

            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_2D, texture1);

            shader.r#use();

            gl::BindVertexArray(vao);
            gl::DrawElements(gl::TRIANGLES, 6, gl::UNSIGNED_INT, ptr::null());
            let (width, height) = window.drawable_size();
            gl::Viewport(0, 0, width as i32, height as i32);
            // gl::Clear(gl::COLOR_BUFFER_BIT);
            // let black = [0., 0., 0., 1.];
            // let name = CString::new("color").unwrap();
            // let color_attr = gl::GetAttribLocation(program_id, name.as_ptr());
            // gl::Uniform4fv(color_attr, 1, mem::transmute(&black));
            // render_text(&state.text_buffer, &mut rast, font_key, 0., 0., 1., 1.)
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

fn render_text(
    text: &str,
    rast: &mut Rasterizer,
    font_key: FontKey,
    mut x: f32,
    mut y: f32,
    sx: f32,
    sy: f32,
) {
    for c in text.chars() {
        let glyph_key = GlyphKey {
            character: c,
            font_key,
            size: Size::new(1.),
        };
        let glyph = rast.get_glyph(glyph_key).unwrap();
        let (buf, format) = match glyph.buffer {
            BitmapBuffer::Rgb(v) => (v, gl::RGB),
            BitmapBuffer::Rgba(v) => (v, gl::RGBA),
        };
        unsafe {
            gl::TexImage2D(
                gl::TEXTURE_2D,
                0,
                gl::RED as i32,
                glyph.width,
                glyph.height,
                0,
                format,
                gl::UNSIGNED_BYTE,
                buf.as_ptr() as *const _,
            )
        }

        let x2 = x + glyph.left as f32 * sx;
        let y2 = -y - glyph.top as f32 * sy;
        let w = glyph.width as f32 * sx;
        let h = glyph.height as f32 * sy;
        for (cnt, byte) in buf.iter().enumerate() {
            if cnt % glyph.width as usize == glyph.width as usize - 1 {
                println!();
            }
            print!("{byte} ");
        }
        let coord_box = [
            [x2, -y2, 0., 0.],
            [x2 + w, -y2, 1., 0.],
            [x2, -y2 - h, 0., 1.],
            [x2 + w, -y2 - h, 1., 1.],
        ];

        unsafe {
            gl::BufferData(
                gl::ARRAY_BUFFER,
                size_of_val(&coord_box) as isize,
                mem::transmute(&coord_box),
                gl::DYNAMIC_DRAW,
            );
            gl::DrawArrays(gl::TRIANGLE_STRIP, 0, 4);
        }

        let (ax, ay) = glyph.advance;
        x += (ax as f32 / 64.) * sx;
        y += (ay as f32 / 64.) * sy;
    }
}

struct TextState {
    text_buffer: String,
    update_text: bool,
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

    fn set_int(&self, name: &str, val: i32) {
        let string = CString::new(name).expect("Name needs to be valid ascii");
        unsafe {
            gl::Uniform1i(gl::GetUniformLocation(self.0, string.as_ptr()), val);
        }
    }
}

fn get_vec_format(buf: BitmapBuffer) -> (Vec<u8>, GLuint) {
    let v = match buf {
        BitmapBuffer::Rgb(v) => v
            .chunks_exact(3)
            .flat_map(|chunk| chunk.iter().copied().chain(Some(255)))
            .collect(),
        BitmapBuffer::Rgba(v) => v,
    };

    (v, gl::RGBA)
}

impl Drop for Shader {
    fn drop(&mut self) {
        unsafe { gl::DeleteProgram(self.0) };
    }
}
