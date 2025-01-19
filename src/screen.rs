use std::{io::Write, mem, path::Path};

use anyhow::{bail, Context};
use byteorder_lite::WriteBytesExt;
use flate2::write::ZlibEncoder;
use image::{GenericImageView, ImageReader, RgbImage};

const ZRLE_TILE_SIZE: u32 = 64;

pub(crate) struct Pointer {
    image: RgbImage,
    bitmask: Box<[u8]>,
}

pub(crate) struct Screen {
    background: RgbImage,
    pub(crate) dimensions: (u16, u16),
    pointer: Option<Pointer>,
}

impl Screen {
    pub(crate) fn create<B, P>(background: B, pointer: Option<P>) -> anyhow::Result<Self>
    where
        B: AsRef<Path>,
        P: AsRef<Path>,
    {
        // Read background
        let background = ImageReader::open(background)
            .context("Read backgroud picture")?
            .decode()
            .context("Decode backgroud picture")?
            .into_rgb8();
        let (width, height) = background.dimensions();
        let width: u16 = width.try_into().context("Width must less than 65536")?;
        let height: u16 = height.try_into().context("Height must less than 65536")?;
        let dimensions = (width, height);

        // Read pointer
        let pointer = match pointer {
            Some(path) => {
                let image = ImageReader::open(path)
                    .context("Read pointer picture")?
                    .decode()
                    .context("Decode pointer picture")?;
                if image.width() > 0xffff || image.height() > 0xffff {
                    bail!("Width & height of poitner picture must less than 65536")
                }
                let rgb888 = image.to_rgb8();
                let rgba = image.into_rgba8();
                let bitmap_row_len = rgba.width().div_ceil(8);
                let mut bitmask = Vec::with_capacity((bitmap_row_len * rgba.height()) as usize);
                for row in rgba.rows() {
                    let mut mask = 0u8;
                    for (i, p) in row.enumerate() {
                        mask = (mask << 1) | (p.0[3] > 0x80) as u8;
                        if i % 8 == 7 {
                            bitmask.push(mask);
                            mask = 0;
                        }
                    }
                    if rgba.width() % 8 > 0 {
                        bitmask.push(mask);
                    }
                }
                Some(Pointer {
                    image: rgb888,
                    bitmask: bitmask.into_boxed_slice(),
                })
            }
            None => None,
        };

        Ok(Self {
            background,
            dimensions,
            pointer,
        })
    }

    pub(crate) fn pointer_size(&self) -> (u16, u16) {
        match self.pointer.as_ref() {
            Some(p) => (p.image.width() as u16, p.image.height() as u16),
            None => (0, 0),
        }
    }

    pub(crate) fn draw_cursor(&self) -> Option<Vec<u8>> {
        let Pointer { image, bitmask } = self.pointer.as_ref()?;
        let mut buf: Vec<_> = image
            .pixels()
            .flat_map(|p| [p.0[2], p.0[1], p.0[0], 0])
            .collect();
        buf.extend_from_slice(bitmask);
        Some(buf)
    }

    pub(crate) fn draw_raw(&self) -> Vec<u8> {
        self.background
            .pixels()
            .flat_map(|p| [p.0[2], p.0[1], p.0[0], 0])
            .collect()
    }

    pub(crate) fn draw_zrle(&self, encoder: &mut ZlibEncoder<Vec<u8>>) -> anyhow::Result<Vec<u8>> {
        let screen_width = self.dimensions.0 as u32;
        let screen_height = self.dimensions.1 as u32;
        let mut buf = [0u8; (ZRLE_TILE_SIZE * ZRLE_TILE_SIZE) as usize * 3];

        for tile_y in 0..screen_height.div_ceil(ZRLE_TILE_SIZE) {
            for tile_x in 0..screen_width.div_ceil(ZRLE_TILE_SIZE) {
                let x = tile_x * ZRLE_TILE_SIZE;
                let y = tile_y * ZRLE_TILE_SIZE;
                let width = ZRLE_TILE_SIZE.clamp(0, screen_width - x);
                let height = ZRLE_TILE_SIZE.clamp(0, screen_height - y);

                let buf = &mut buf[..(width * height * 3) as usize];
                self.background
                    .view(x, y, width, height)
                    .pixels()
                    .zip(buf.chunks_mut(3))
                    .for_each(|((_, _, p), b)| {
                        b[0] = p.0[2];
                        b[1] = p.0[1];
                        b[2] = p.0[0];
                    });
                encoder.write_u8(0).unwrap(); // no RLE, no palette
                encoder.write_all(buf).unwrap();
            }
        }

        encoder.flush()?;
        let buf = mem::take(encoder.get_mut());
        Ok(buf)
    }
}
