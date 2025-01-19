use std::{io::Write, mem, path::Path};

use anyhow::Context;
use byteorder_lite::WriteBytesExt;
use flate2::write::ZlibEncoder;
use image::{GenericImageView, ImageReader, RgbImage};

const ZRLE_TILE_SIZE: u32 = 64;

pub(crate) struct Screen {
    background: RgbImage,
    pub(crate) dimensions: (u16, u16),
}

impl Screen {
    pub(crate) fn create<P>(background: P) -> anyhow::Result<Self>
    where
        P: AsRef<Path>,
    {
        let background = ImageReader::open(background)
            .context("Read backgroud picture")?
            .decode()
            .context("Decode backgroud picture")?
            .into_rgb8();
        let (width, height) = background.dimensions();
        let width: u16 = width.try_into().context("Width must less than 65536")?;
        let height: u16 = height.try_into().context("Height must less than 65536")?;
        let dimensions = (width, height);

        Ok(Self {
            background,
            dimensions,
        })
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
