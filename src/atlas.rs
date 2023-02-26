use gl::types::{GLfloat, GLuint};
use std::{collections::HashMap, iter::repeat};

use crossfont::{
    BitmapBuffer, Error, FontKey, GlyphKey, Rasterize, RasterizedGlyph, Rasterizer, Size,
};

#[derive(Clone, Copy)]
struct RGBA([u8; 4]);

/// Represents where a glyph is in memory
#[derive(Clone, Copy)]
struct AtlasIndex {
    y_index: usize,
    top: f32,
    left: f32,
    width: f32,
    height: f32,
    ax: f32,
    ay: f32,
}

pub struct GlyphAtlas {
    /// Stores the glyphs
    pixel_buffer: Vec<RGBA>,
    buffer_width: usize,
    glyphs: HashMap<char, AtlasIndex>,
    font_key: FontKey,
    /// Position of the "unknown character" glyph
    unknown_position: AtlasIndex,
    scale: f32,
}

impl GlyphAtlas {
    /// How large the internal vector's rows should be, compared to the first
    /// character generated. This is mostly to avoid having to write resizing
    /// code. And yes, 10 is absolutely overkill.
    const MAX_WIDTH_RATIO: usize = 3;
    pub fn new(
        rasteriser: &mut Rasterizer,
        font_key: FontKey,
        texture1: GLuint,
        camera_scale: u32,
    ) -> Result<Self, Error> {
        rasteriser.update_dpr(camera_scale as f32);
        let glyph = get_glyph(rasteriser, font_key, '?')?;
        let buffer_width = glyph.width as usize * Self::MAX_WIDTH_RATIO;

        let mut pixel_buffer = Vec::new();
        let scale = 1. / camera_scale as f32;
        let unknown_position = push_pixels(glyph, &mut pixel_buffer, buffer_width, scale);

        let mut res = Self {
            scale,
            pixel_buffer,
            buffer_width,
            glyphs: HashMap::new(),
            font_key,
            unknown_position,
        };

        let printable_ascii = (32..127_u8).map(|b| b as char);
        res.add_characters(printable_ascii, texture1, rasteriser);

        Ok(res)
    }
    fn buffer_height(&self) -> usize {
        self.pixel_buffer.len() / self.buffer_width
    }

    pub fn add_characters<I: Iterator<Item = char>>(&mut self, chars: I, texture1: GLuint, rast: &mut Rasterizer) {
        let num_glyphs_before = self.glyphs.len();
        for c in chars {
            if self.glyphs.contains_key(&c) {
                continue;
            }

            let glyph = match get_glyph(rast, self.font_key, c) {
                Err(e) => {
                    eprintln!("Couldn't rasterise character {c}: {e}");
                    return;
                }
                Ok(g) => g,
            };

            let glyph_info =
                push_pixels(glyph, &mut self.pixel_buffer, self.buffer_width, self.scale);
            self.glyphs.insert(c, glyph_info);
        }

        if num_glyphs_before != self.glyphs.len() {
            unsafe { self.upload_texture(texture1) }
        }
    }

    // TODO: remove this function (and integrate it somewhere else)
    pub unsafe fn upload_texture(&self, texture1: GLuint) {
        gl::ActiveTexture(gl::TEXTURE0);
        gl::BindTexture(gl::TEXTURE_2D, texture1);

        let flattened: Vec<_> = self
            .pixel_buffer
            .iter()
            .flat_map(|rgba| rgba.0.iter().copied())
            .collect();

        let fl = flattened.len();

        assert!(fl == 4 * self.pixel_buffer.len());

        gl::TexImage2D(
            gl::TEXTURE_2D,
            0,
            gl::RED as i32,
            self.buffer_width as i32,
            self.buffer_height() as i32,
            0,
            gl::RGBA,
            gl::UNSIGNED_BYTE,
            self.pixel_buffer.as_ptr() as *const _,
        );
    }

    pub fn measure_dims<I: Iterator<Item = char>>(&self, chars: I) -> (f32, f32) {
        chars
            .map(|c| self.glyphs.get(&c).unwrap_or(&self.unknown_position))
            .fold((0.0, 0.0), |(x, y), g| (x + g.ax, y + g.ay))
    }

    pub fn get_glyph_data(
        &self,
        c: char,
        x0: GLfloat,
        y0: GLfloat,
    ) -> ([[GLfloat; 4]; 4], f32, f32) {
        let pos = self.glyphs.get(&c).unwrap_or(&self.unknown_position);

        let top = pos.top as f32;
        let left = pos.left as f32;
        let width = pos.width as f32;
        let height = pos.height as f32;

        let x1 = x0 + left;
        let x2 = x1 + width;

        let y1 = y0 - top;
        let y2 = y1 + height;

        let num_lines = (self.pixel_buffer.len() / self.buffer_width) as f32;
        let t_top = pos.y_index as f32 / num_lines;
        // TODO: find a less awkward way to do this
        let t_bottom = t_top + height / self.scale / num_lines;

        let s_left = 0.;
        let s_right = width / self.scale / self.buffer_width as f32;

        let verts = [
            //positions      // texture coordinates
            [x2, y1, s_right, t_top],    // top right
            [x2, y2, s_right, t_bottom], // bottom right
            [x1, y2, s_left, t_bottom],  // bottom left
            [x1, y1, s_left, t_top],     // top left
        ];

        (verts, pos.ax, pos.ay)
    }
}

fn get_glyph(
    rasteriser: &mut Rasterizer,
    font_key: FontKey,
    c: char,
) -> Result<RasterizedGlyph, Error> {
    let glyph_key = GlyphKey {
        character: c,
        font_key,
        size: Size::new(1.),
    };
    rasteriser.get_glyph(glyph_key)
}

fn expand_width(bitmap: Vec<RGBA>, src_width: usize, dest_width: usize) -> Vec<RGBA> {
    debug_assert!(src_width <= dest_width);
    if src_width == 0 {
        return vec![RGBA([0x00; 4]); dest_width];
    }
    bitmap
        .chunks_exact(src_width)
        .enumerate()
        .flat_map(|(_, orig_row)| {
            orig_row
                .iter()
                .copied()
                .chain(repeat(RGBA([0x00; 4])))
                .take(dest_width)
        })
        .collect()
}

fn push_pixels(
    glyph: RasterizedGlyph,
    pixel_buffer: &mut Vec<RGBA>,
    buffer_width: usize,
    scale: f32,
) -> AtlasIndex {
    // Transform into rgba (there is a bug with opengl that treats all
    // input textures as rgba)
    let pixels = match glyph.buffer {
        BitmapBuffer::Rgb(v) => v
            .chunks_exact(3)
            .map(|chunk| {
                let mut res = [0xff; 4];
                for (res_el, &chunk_el) in res.iter_mut().zip(chunk) {
                    *res_el = chunk_el
                }
                RGBA(res)
            })
            .collect(),
        BitmapBuffer::Rgba(v) => v
            .chunks_exact(4)
            .map(|slice| RGBA(slice.try_into().expect("We used a chunk size of 4")))
            .collect(),
    };
    let new_pixels: Vec<_> = expand_width(pixels, glyph.width as usize, buffer_width);
    let y_index = pixel_buffer.len() / buffer_width;
    pixel_buffer.extend(new_pixels);
    let (ax, ay) = glyph.advance;
    AtlasIndex {
        y_index,
        top: glyph.top as f32 * scale,
        left: glyph.left as f32 * scale,
        width: glyph.width as f32 * scale,
        height: glyph.height as f32 * scale,
        ax: ax as f32 * scale,
        ay: ay as f32 * scale,
    }
}
