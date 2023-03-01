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
    top: f64,
    left: f64,
    width: f64,
    height: f64,
    ax: f64,
    ay: f64,
}

pub struct GlyphAtlas {
    /// Stores the glyphs
    pixel_buffer: Vec<RGBA>,
    buffer_width: usize,
    glyphs: HashMap<char, AtlasIndex>,
    font_key: FontKey,
    /// Position of the "unknown character" glyph
    unknown_position: AtlasIndex,
    scale: f64,
    line_height: f64,
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
        rasteriser.update_dpr(Size::factor() * camera_scale as f32);
        let glyph = get_glyph(rasteriser, font_key, '?')?;
        let buffer_width = glyph.width as usize * Self::MAX_WIDTH_RATIO;

        let mut pixel_buffer = Vec::new();
        let scale = 1. / camera_scale as f64;
        let unknown_position = push_pixels(glyph, &mut pixel_buffer, buffer_width, scale);
        let metrics = rasteriser.metrics(font_key, Size::new(1.))?;

        let mut res = Self {
            scale,
            pixel_buffer,
            buffer_width,
            glyphs: HashMap::new(),
            font_key,
            line_height: metrics.line_height as f64 * scale,
            unknown_position,
        };

        let printable_ascii = (32..127_u8).map(|b| b as char);
        res.add_characters(printable_ascii, texture1, rasteriser);

        Ok(res)
    }

    fn buffer_height(&self) -> usize {
        self.pixel_buffer.len() / self.buffer_width
    }

    pub fn line_height(&self) -> f64 {
        self.line_height
    }

    pub fn add_characters<I: Iterator<Item = char>>(
        &mut self,
        chars: I,
        texture1: GLuint,
        rast: &mut Rasterizer,
    ) {
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

        // let next_buf_height = self.buffer_height().next_power_of_two();
        // let to_pad = next_buf_height - self.buffer_height();
        // self.pixel_buffer.extend(repeat(RGBA([0; 4])).take(self.buffer_width * to_pad));

        // assert!(self.buffer_height().is_power_of_two(), "{}", self.buffer_height());

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

        // TODO: make sure buffer height doesn't go above gl::MAX_TEX_LAYERs or somn
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

    pub fn measure_dims<I: Iterator<Item = char>>(&self, chars: I) -> (f64, f64) {
        let (w, h) = chars
            .take_while(|c| *c != '\n')
            .map(|c| self.glyphs.get(&c).unwrap_or(&self.unknown_position))
            .fold((0.0, 0.0), |(x, y), g| (x + g.ax, y + g.ay));

        (w, h + self.line_height)
    }

    pub fn get_glyph_data(
        &self,
        c: char,
        x0: f64,
        y0: f64,
    ) -> ([[GLfloat; 4]; 4], f64, f64) {
        let pos = self.glyphs.get(&c).unwrap_or(&self.unknown_position);

        let top = pos.top as f64;
        let left = pos.left as f64;
        let width = pos.width as f64;
        let height = pos.height as f64;

        let x1 = x0 + left;
        let x2 = x1 + width;

        let y1 = y0 - top;
        let y2 = y1 + height;

        let num_lines = self.buffer_height() as f64;
        let t1 = pos.y_index as f64 / num_lines;
        // TODO: find a less awkward way to do this
        let t2 = t1 + (height / self.scale) / num_lines;

        let s1 = 0.;
        let s2 = width / self.scale / self.buffer_width as f64;

        let verts = [
            //positions      // texture coordinates
            [x2 as f32, y1 as f32, s2 as f32, t1 as f32], // top right
            [x2 as f32, y2 as f32, s2 as f32, t2 as f32], // bottom right
            [x1 as f32, y2 as f32, s1 as f32, t2 as f32], // bottom left
            [x1 as f32, y1 as f32, s1 as f32, t1 as f32], // top left
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
    scale: f64,
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
    pixel_buffer.extend(repeat(RGBA([0; 4])).take(buffer_width));
    let (ax, ay) = glyph.advance;
    AtlasIndex {
        y_index,
        top: glyph.top as f64 * scale,
        left: glyph.left as f64 * scale,
        width: glyph.width as f64 * scale,
        height: glyph.height as f64 * scale,
        ax: ax as f64 * scale,
        ay: ay as f64 * scale,
    }
}
