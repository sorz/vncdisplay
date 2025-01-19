use std::path::Path;

use anyhow::Context;
use image::{ImageReader, RgbImage};

pub(crate) struct Screen {
    background: RgbImage,
    pub(crate) dimensions: (u16, u16),
    pub(crate) framebuffer_raw: Vec<u8>,
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
            framebuffer_raw: Vec::new(),
        })
    }
}
