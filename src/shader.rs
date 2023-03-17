use std::{ffi::CString, mem, ptr, str};

use gl::types::{GLenum, GLfloat, GLint, GLuint};

use crate::check_err;

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

pub struct Shader<const N: usize> {
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
    pub fn upload_rectangles(&self, data: &[[[f32; N]; 4]]) {
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
            let elem_array_size = mem::size_of_val(indices.as_slice());
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
                mem::size_of_val(data) as isize,
                data.as_ptr().cast(),
                gl::DYNAMIC_DRAW,
            );
        }
    }

    pub fn r#use(&self) {
        unsafe {
            gl::UseProgram(self.program_id);
        }
    }

    // TODO: make these panic when the name is invalid
    pub fn uniform1i(&self, name: &str, val: i32) {
        let name = c_str(name);
        unsafe {
            gl::Uniform1i(gl::GetUniformLocation(self.program_id, name.as_ptr()), val);
        }
    }

    pub fn uniform1f(&self, name: &str, val: GLfloat) {
        let name = c_str(name);
        unsafe {
            gl::Uniform1f(gl::GetUniformLocation(self.program_id, name.as_ptr()), val);
        }
    }

    pub fn uniform4vf(&self, name: &str, val: [GLfloat; 4]) {
        let name = c_str(name);
        unsafe {
            gl::Uniform4fv(
                gl::GetUniformLocation(self.program_id, name.as_ptr()),
                1,
                val.as_ptr(),
            );
        }
    }

    pub fn uniform2i(&self, name: &str, val: [GLint; 2]) {
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
    pub fn text_shader(vbo: GLuint) -> Self {
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
    pub fn shape_shader(vbo: GLuint) -> Self {
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
