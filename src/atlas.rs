use gl::types::{GLfloat, GLuint};
use std::{
    cell::{Ref, RefCell, RefMut},
    collections::{BTreeMap, HashMap},
    iter::repeat,
    ops::{Deref, DerefMut},
};

use crossfont::{
    ft::fc::{Config, SetName},
    BitmapBuffer, Error, FontDesc, FontKey, GlyphKey, Rasterize, RasterizedGlyph, Rasterizer, Size,
    Slant, Style, Weight,
};

use crate::shader::Shader;

#[derive(Clone, Copy)]
struct Rgba([u8; 4]);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Font<'a>(&'a str);
impl<'a> std::fmt::Display for Font<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl<'a> Font<'a> {
    pub fn query() -> impl Iterator<Item = Font<'a>> {
        // TODO: get a different abstraction layer
        let ft_cfg = Config::get_current();
        let sys_fonts = ft_cfg.get_fonts(SetName::System);
        sys_fonts
            .into_iter()
            .flat_map(|font| font.fullname())
            .filter(|s| {
                let lowercase = s.to_lowercase();
                lowercase.contains("mono")
                    && !lowercase.contains("italic")
                    && !lowercase.contains("oblique")
                    && !lowercase.contains("bold")
            })
            .map(Font)
    }
}

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
    /// Contains the computed sizes
    sizes: RefCell<BTreeMap<u32, GlyphMap>>,
    rasteriser: RefCell<Rasterizer>,
    font_key: FontKey,
    texture1: GLuint,
    /// Determines the factor TODO: explain these better
    current_scale: u32,
    /// Determines how big the letters will be on screen
    letter_size: u32,
}

impl GlyphAtlas {
    pub const SCALE_STEP: f32 = 1. / 32.;
    pub const MIN_SCALE: u32 = (4. / Self::SCALE_STEP) as u32;
    fn get_current(&self) -> impl Deref<Target = GlyphMap> + '_ {
        self.sizes
            .borrow_mut()
            .entry(self.current_scale)
            .or_insert_with(|| {
                let new_gmap = GlyphMap::new(
                    &mut self.rasteriser.borrow_mut(),
                    self.font_key,
                    self.current_scale as f32 * Self::SCALE_STEP,
                )
                .unwrap(); // TODO: figure out how to handle errors
                new_gmap
            });

        Ref::map(self.sizes.borrow(), |sizes| {
            sizes
                .get(&self.current_scale)
                .expect("Key should be present")
        })
    }

    fn get_current_mut(&self) -> impl DerefMut<Target = GlyphMap> + '_ {
        self.sizes
            .borrow_mut()
            .entry(self.current_scale)
            .or_insert_with(|| {
                let new_gmap = GlyphMap::new(
                    &mut self.rasteriser.borrow_mut(),
                    self.font_key,
                    self.current_scale as f32 * Self::SCALE_STEP,
                )
                .unwrap(); // TODO: figure out how to handle errors
                new_gmap
            });

        RefMut::map(self.sizes.borrow_mut(), |sizes| {
            sizes
                .get_mut(&self.current_scale)
                .expect("Key should be present")
        })
    }

    pub fn new(text_shader: &Shader<4>) -> Self {
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
        let mut rasteriser = Rasterizer::new(1.).expect("Could not set up rasterizer");
        let font_desc = FontDesc::new(
            "Liberation Mono",
            Style::Description {
                slant: Slant::Normal,
                weight: Weight::Normal,
            },
        );
        let font_key = rasteriser
            .load_font(&font_desc, Size::new(0.))
            .expect("Could not load font");
        Self {
            sizes: RefCell::new(BTreeMap::new()),
            rasteriser: RefCell::new(rasteriser),
            font_key,
            texture1,
            current_scale: Self::MIN_SCALE,
            letter_size: 2,
        }
    }

    pub fn change_font(&mut self, font: Font) {
        self.sizes.replace(BTreeMap::new());
        let font_desc = FontDesc::new(
            font.0,
            Style::Description {
                slant: Slant::Normal,
                weight: Weight::Normal,
            },
        );
        let font_key = self
            .rasteriser
            .borrow_mut()
            .load_font(&font_desc, Size::new(0.))
            .expect("Font was found previously");

        self.font_key = font_key;
        unsafe { GlyphMap::upload_texture(&self.get_current(), self.texture1) };
    }

    pub fn select_scale(&mut self, scale: f32, letter_size: u32) -> f32 {
        self.letter_size = letter_size;
        let scale_rounded = (letter_size as f32 * scale / Self::SCALE_STEP).round() as u32;
        let prev_scale = self.current_scale;
        if prev_scale != scale_rounded {
            self.current_scale = scale_rounded;
            unsafe { GlyphMap::upload_texture(&self.get_current(), self.texture1) };
        }
        prev_scale as f32 * Self::SCALE_STEP
    }

    pub fn add_characters<I: Iterator<Item = char>>(&mut self, chars: I) {
        let mut map = self.get_current_mut();
        let old_height = GlyphMap::buffer_height(&map);
        map.add_characters(chars, &mut self.rasteriser.borrow_mut());
        let new_height = GlyphMap::buffer_height(&map);
        if old_height != new_height {
            unsafe {
                GlyphMap::upload_texture(&map, self.texture1);
            }
        }
    }

    pub fn line_height(&mut self) -> f32 {
        self.get_current().line_height * self.letter_size as f32
    }

    pub fn measure_dims(&self, chars: impl Iterator<Item = char>) -> (f32, f32) {
        // Call to make sure at least one value is in the map
        self.get_current();
        let maps = self.sizes.borrow();
        let biggest = maps
            .last_key_value()
            .expect("At least one entry was just inserted")
            .1;
        let (w, h) = biggest.measure_dims(chars);
        let s = self.letter_size as f32;
        (w * s, h * s)
    }

    pub fn get_glyph_data(&mut self, c: char, x0: f32, y0: f32) -> ([[GLfloat; 4]; 4], f32, f32) {
        self.get_current()
            .get_glyph_data(c, x0, y0, self.letter_size as f32)
    }

    pub fn ascender(&self) -> f32 {
        self.get_current().ascender * self.letter_size as f32
    }

    pub fn descender(&self) -> f32 {
        self.get_current().descender * self.letter_size as f32
    }
}

struct GlyphMap {
    /// Stores the glyphs
    pixel_buffer: Vec<Rgba>,
    buffer_width: usize,
    glyphs: HashMap<char, AtlasIndex>,
    font_key: FontKey,
    /// Position of the "unknown character" glyph
    unknown_position: AtlasIndex,
    camera_scale: f32,
    line_height: f32,
    ascender: f32,
    descender: f32,
}

impl GlyphMap {
    /// How large the internal vector's rows should be, compared to the first
    /// character generated. self is mostly to avoid having to write resizing
    /// code. And yes, 10 is absolutely overkill.
    const MAX_WIDTH_RATIO: usize = 3;
    pub fn new(
        rasteriser: &mut Rasterizer,
        font_key: FontKey,
        camera_scale: f32,
    ) -> Result<Self, Error> {
        rasteriser.update_dpr(Size::factor() * camera_scale);
        let glyph = get_glyph(rasteriser, font_key, '?')?;
        let buffer_width = glyph.width as usize * Self::MAX_WIDTH_RATIO;

        let mut pixel_buffer = Vec::new();
        let scale = 1. / camera_scale;
        let unknown_position = push_pixels(glyph, &mut pixel_buffer, buffer_width, scale);
        let metrics = rasteriser.metrics(font_key, Size::new(1.))?;

        let line_height = metrics.line_height as f32 * scale;
        let descender = metrics.descent * scale;
        let mut res = Self {
            camera_scale: scale,
            pixel_buffer,
            buffer_width,
            glyphs: HashMap::new(),
            font_key,
            line_height,
            descender,
            ascender: descender + line_height,
            unknown_position,
        };

        let printable_ascii = (32..127_u8).map(|b| b as char);
        res.add_characters(printable_ascii, rasteriser);

        Ok(res)
    }

    fn buffer_height(&self) -> usize {
        self.pixel_buffer.len() / self.buffer_width
    }

    pub fn add_characters<I: Iterator<Item = char>>(&mut self, chars: I, rast: &mut Rasterizer) {
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

            let glyph_info = push_pixels(
                glyph,
                &mut self.pixel_buffer,
                self.buffer_width,
                self.camera_scale,
            );
            self.glyphs.insert(c, glyph_info);
        }
    }
    pub unsafe fn upload_texture(&self, texture1: GLuint) {
        gl::ActiveTexture(gl::TEXTURE0);
        gl::BindTexture(gl::TEXTURE_2D, texture1);

        let flattened = self
            .pixel_buffer
            .iter()
            .flat_map(|rgba| rgba.0.iter().copied());

        let fl = flattened.count();

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
            self.pixel_buffer.as_ptr().cast(),
        );
    }

    pub fn measure_dims<I: Iterator<Item = char>>(&self, chars: I) -> (f32, f32) {
        let (w, h) = chars
            .take_while(|c| *c != '\n')
            .map(|c| self.glyphs.get(&c).unwrap_or(&self.unknown_position))
            .fold((0.0, 0.0), |(x, y), g| (x + g.ax, y + g.ay));

        (w, h + self.line_height)
    }

    pub fn get_glyph_data(
        &self,
        c: char,
        x0: f32,
        y0: f32,
        letter_scale: f32,
    ) -> ([[GLfloat; 4]; 4], f32, f32) {
        let pos = self.glyphs.get(&c).unwrap_or(&self.unknown_position);

        let top = pos.top;
        let left = pos.left;
        let width = pos.width;
        let height = pos.height;

        let x1 = x0 + left * letter_scale;
        let x2 = x1 + width * letter_scale;

        let y1 = y0 - top * letter_scale;
        let y2 = y1 + height * letter_scale;

        let num_lines = self.buffer_height() as f32;
        let t1 = pos.y_index as f32 / num_lines;
        // TODO: find a less awkward way to do this
        let t2 = t1 + (height / self.camera_scale) / num_lines;

        let s1 = 0.;
        let s2 = width / self.camera_scale / self.buffer_width as f32;

        let verts = [
            //positions      // texture coordinates
            [x2, y1, s2, t1], // top right
            [x2, y2, s2, t2], // bottom right
            [x1, y2, s1, t2], // bottom left
            [x1, y1, s1, t1], // top left
        ];

        (verts, pos.ax * letter_scale, pos.ay * letter_scale)
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

fn expand_width(bitmap: Vec<Rgba>, src_width: usize, dest_width: usize) -> Vec<Rgba> {
    debug_assert!(src_width <= dest_width);
    if src_width == 0 {
        return vec![Rgba([0x00; 4]); dest_width];
    }
    bitmap
        .chunks_exact(src_width)
        .enumerate()
        .flat_map(|(_, orig_row)| {
            orig_row
                .iter()
                .copied()
                .chain(repeat(Rgba([0x00; 4])))
                .take(dest_width)
        })
        .collect()
}

fn push_pixels(
    glyph: RasterizedGlyph,
    pixel_buffer: &mut Vec<Rgba>,
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
                    *res_el = chunk_el;
                }
                Rgba(res)
            })
            .collect(),
        BitmapBuffer::Rgba(v) => v
            .chunks_exact(4)
            .map(|slice| Rgba(slice.try_into().expect("We used a chunk size of 4")))
            .collect(),
    };
    let new_pixels: Vec<_> = expand_width(pixels, glyph.width as usize, buffer_width);
    let y_index = pixel_buffer.len() / buffer_width;
    pixel_buffer.extend(new_pixels);
    pixel_buffer.extend(repeat(Rgba([0; 4])).take(buffer_width));
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
