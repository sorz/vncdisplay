use std::io::{self, Read, Write};

use anyhow::{bail, Context};
use byteorder_lite::{ReadBytesExt, WriteBytesExt, BE, LE};
use image::Rgb;
use log::debug;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

static SECURITY_TYPE_NO_AUTHENTICATION: u8 = 1;
static SECURITY_RESULT_OK: u32 = 0;
static SECURITY_RESULT_FAILED: u32 = 1;

static ERROR_REASON_PROTOCOL_VERSION_UNSUPPORTED: &str = "Unsupported protocol version";
static ERROR_REASON_SECURITY_TYPE_UNSUPPORTED: &str = "Unsupported security type";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum RfpVersion {
    V3_3,
    V3_7,
    V3_8,
}

/// RFC6143 §7.4. Pixel Format Data Structure
#[derive(Debug, Clone, Copy)]
pub(crate) struct PixelFormat {
    pub(crate) bits_per_pixel: u8,
    pub(crate) depth: u8,
    pub(crate) big_endian_flag: bool,
    pub(crate) true_color_flag: bool,
    pub(crate) red_max: u16,
    pub(crate) green_max: u16,
    pub(crate) blue_max: u16,
    pub(crate) red_shift: u8,
    pub(crate) green_shift: u8,
    pub(crate) blue_shift: u8,
}

static PIXEL_FOMRAT_RGB888: &PixelFormat = &PixelFormat {
    bits_per_pixel: 32,
    depth: 24,
    big_endian_flag: false,
    true_color_flag: true,
    red_max: 0xff,
    green_max: 0xff,
    blue_max: 0xff,
    red_shift: 16,
    green_shift: 8,
    blue_shift: 0,
};

impl Default for PixelFormat {
    fn default() -> Self {
        *PIXEL_FOMRAT_RGB888
    }
}

impl PixelFormat {
    fn read_from<R: Read>(reader: &mut R) -> anyhow::Result<Self> {
        let format = PixelFormat {
            bits_per_pixel: reader.read_u8()?,
            depth: reader.read_u8()?,
            big_endian_flag: reader.read_u8()? > 0,
            true_color_flag: reader.read_u8()? > 0,
            red_max: reader.read_u16::<BE>()?,
            green_max: reader.read_u16::<BE>()?,
            blue_max: reader.read_u16::<BE>()?,
            red_shift: reader.read_u8()?,
            green_shift: reader.read_u8()?,
            blue_shift: reader.read_u8()?,
        };
        // 7.4.  Pixel Format Data Structure
        // bits-per-pixel must be 8, 16, or 32
        match format.bits_per_pixel {
            8 | 16 | 32 => (),
            _ => bail!("bits-per-pixel must be 8, 16, or 32"),
        }
        if format.depth > format.bits_per_pixel {
            bail!("depth exceeds bits_per_pixel")
        }
        Ok(format)
    }

    pub(crate) fn bytes_per_pixel(&self) -> usize {
        self.bits_per_pixel as usize / 8
    }

    pub(crate) fn encode_compressed_pixels<P, W>(
        &self,
        pixels: P,
        writer: &mut W,
    ) -> anyhow::Result<()>
    where
        P: Iterator<Item = Rgb<u8>>,
        W: Write,
    {
        // 7.7.5. TRLE
        // Check eligibility
        if !self.true_color_flag || self.bits_per_pixel != 32 || self.depth > 24 {
            // FIXME: check bitmask
            // Fallback to uncompressed pixels
            return self.encode_pixels(pixels, writer);
        }
        // Use compressed pxiel format
        if self.depth != 24 {
            bail!("Unimplemented: color depth within (16, 24)");
        }
        for Rgb([r, g, b]) in pixels {
            if self.big_endian_flag {
                writer.write_all(&[r, g, b])?;
            } else {
                writer.write_all(&[b, g, r])?;
            }
        }
        Ok(())
    }

    pub(crate) fn encode_pixels<P, W>(&self, pixels: P, writer: &mut W) -> anyhow::Result<()>
    where
        P: Iterator<Item = Rgb<u8>>,
        W: Write,
    {
        let rgb_max = [
            self.red_max as u32,
            self.green_max as u32,
            self.blue_max as u32,
        ];
        if !self.true_color_flag {
            bail!("Unimplemented: true color only")
        }
        let rgb_shift = [self.red_shift, self.green_shift, self.blue_shift];
        for Rgb(rgb) in pixels {
            let mut pixel = 0u32;
            for i in 0..3 {
                pixel |= (rgb[i] as u32 * rgb_max[i] / 0xff) << rgb_shift[i]
            }
            match self.bits_per_pixel {
                8 => writer.write_u8(pixel as u8)?,
                16 if self.big_endian_flag => writer.write_u16::<BE>(pixel as u16)?,
                16 => writer.write_u16::<LE>(pixel as u16)?,
                32 if self.big_endian_flag => writer.write_u32::<BE>(pixel)?,
                32 => writer.write_u32::<LE>(pixel)?,
                _ => bail!("bits_per_pixel must be 8, 16, or 32"),
            }
        }
        Ok(())
    }
}

/// RFC6143 §7.5. Client-to-Server Messages
#[derive(Debug, Clone)]
pub(crate) enum ClientMessage {
    SetPixelFormat(PixelFormat),
    SetEncodings(Vec<Encoding>),
    FramebufferUpdateRequest {
        incremental: bool,
        position: (u16, u16),
        size: (u16, u16),
    },
    KeyEvent,
    PointerEvent,
    ClientCutText,
}

/// RFC6143 §8.4. RFB Encoding Types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Encoding {
    Raw,    // 0
    Zrle,   // 16
    Cursor, // -239
    Other(i32),
}

impl From<i32> for Encoding {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Raw,
            16 => Self::Zrle,
            -239 => Self::Cursor,
            n => Self::Other(n),
        }
    }
}

impl From<Encoding> for i32 {
    fn from(val: Encoding) -> Self {
        match val {
            Encoding::Raw => 0,
            Encoding::Zrle => 16,
            Encoding::Cursor => -239,
            Encoding::Other(value) => value,
        }
    }
}

pub(crate) struct FrameRectangle {
    position: (u16, u16),
    size: (u16, u16),
    encoding: Encoding,
    buf: Vec<u8>,
}

impl FrameRectangle {
    pub(crate) fn new_raw_frame(size: (u16, u16), buf: Vec<u8>) -> Self {
        Self {
            position: (0, 0),
            encoding: Encoding::Raw,
            size,
            buf,
        }
    }

    pub(crate) fn new_zrle_frame(size: (u16, u16), buf: Vec<u8>) -> Self {
        Self {
            position: (0, 0),
            encoding: Encoding::Zrle,
            size,
            buf,
        }
    }

    pub(crate) fn new_cursor(size: (u16, u16), buf: Vec<u8>) -> Self {
        Self {
            position: (size.0 / 2, size.1 / 2),
            size,
            encoding: Encoding::Cursor,
            buf,
        }
    }
}

/// Handshake with client.
/// From TCP connection established to initialization messages exchanged.
pub(crate) async fn handshake(
    stream: &mut TcpStream,
    screen_dimensions: (u16, u16),
    name: &str,
) -> anyhow::Result<()> {
    // RFC 6143: The Remote Framebuffer Protocol
    // 7.1.1. ProtocolVersion Handshake
    stream
        .write_all(b"RFB 003.008\n")
        .await
        .context("Send server protocol version")?;
    let mut buf = [0u8; 12];
    stream
        .read_exact(buf.as_mut_slice())
        .await
        .context("Read client protocol version")?;
    let version = match &buf {
        b"RFB 003.008\n" => RfpVersion::V3_8,
        b"RFB 003.007\n" => RfpVersion::V3_7,
        b"RFB 003.003\n" => RfpVersion::V3_3,
        _ => {
            stream.write_u8(0).await?;
            stream
                .write_u32(
                    ERROR_REASON_PROTOCOL_VERSION_UNSUPPORTED
                        .len()
                        .try_into()
                        .unwrap(),
                )
                .await?;
            stream
                .write_all(ERROR_REASON_PROTOCOL_VERSION_UNSUPPORTED.as_bytes())
                .await?;
            bail!("Unknown client protocol version: {:?}", buf);
        }
    };
    debug!("Protocol version handshake finish: {:?}", version);

    // 7.1.2. Security Handshake
    let secuirty_type = if version == RfpVersion::V3_3 {
        // A.1. Differences in the Version 3.3 Protocol
        stream
            .write_u32(SECURITY_TYPE_NO_AUTHENTICATION as u32)
            .await?;
        SECURITY_TYPE_NO_AUTHENTICATION
    } else {
        // Two-way negotiation for V3.7 & V3.8
        stream
            .write_all(&[1, SECURITY_TYPE_NO_AUTHENTICATION])
            .await?;
        stream.read_u8().await?
    };
    match version {
        RfpVersion::V3_3 => (), // No checking
        RfpVersion::V3_7 if secuirty_type == SECURITY_TYPE_NO_AUTHENTICATION => (), // No SecurityResult
        RfpVersion::V3_8 if secuirty_type == SECURITY_TYPE_NO_AUTHENTICATION => {
            // Send SecurityResult (OK)
            stream.write_u32(SECURITY_RESULT_OK).await?;
        }
        _ => {
            // Send SecurityResult (FAILED)
            stream.write_u32(SECURITY_RESULT_FAILED).await?;
            if version == RfpVersion::V3_8 {
                // Send reason
                stream
                    .write_u32(
                        ERROR_REASON_SECURITY_TYPE_UNSUPPORTED
                            .len()
                            .try_into()
                            .unwrap(),
                    )
                    .await?;
                stream
                    .write_all(ERROR_REASON_SECURITY_TYPE_UNSUPPORTED.as_bytes())
                    .await?;
                bail!("Unsupported security type: {}", secuirty_type);
            }
        }
    }

    // 7.3.1. ClientInit
    let shared = stream.read_u8().await? > 0;
    debug!("Client request shared_flag = {}", shared);
    // Ignored, we always do sharing

    // 7.3.2. ServerInit
    stream.write_u16(screen_dimensions.0).await?; // width
    stream.write_u16(screen_dimensions.1).await?; // height
    stream.write_all(&PIXEL_FOMRAT_RGB888.encode()).await?;
    let name_len: u32 = name.len().try_into().unwrap_or(u32::MAX);
    stream.write_u32(name_len).await?;
    stream
        .write_all(&name.as_bytes()[..name_len as usize])
        .await?;
    Ok(())
}

pub(crate) async fn read_message(
    stream: &mut TcpStream,
    buf: &mut Vec<u8>,
) -> anyhow::Result<Option<ClientMessage>> {
    let msg = match stream.read_u8().await {
        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(err) => return Err(err.into()),
        Ok(0) => {
            // SetPixelFormat
            buf.resize(3 + 16, 0);
            stream.read_exact(buf).await?;
            let mut reader = &buf[3..];
            let format = PixelFormat::read_from(&mut reader)?;
            ClientMessage::SetPixelFormat(format)
        }
        Ok(2) => {
            // SetEncodings
            stream.read_u8().await?; // padding
            let len: usize = stream.read_u16().await?.into();
            buf.resize(len * 4, 0);
            stream.read_exact(buf).await?;
            let encodings: Vec<Encoding> = buf
                .as_slice()
                .chunks(4)
                .map(|b| i32::from_be_bytes(b.try_into().unwrap()).into())
                .collect();
            ClientMessage::SetEncodings(encodings)
        }
        Ok(3) => {
            // FramebufferUpdateRequest
            buf.resize(1 + 2 + 2 + 2 + 2, 0);
            stream.read_exact(buf).await?;
            ClientMessage::FramebufferUpdateRequest {
                incremental: buf[0] > 0,
                position: (
                    u16::from_be_bytes([buf[1], buf[2]]),
                    u16::from_be_bytes([buf[3], buf[4]]),
                ),
                size: (
                    u16::from_be_bytes([buf[5], buf[6]]),
                    u16::from_be_bytes([buf[7], buf[8]]),
                ),
            }
        }
        Ok(4) => {
            // KeyEvent
            buf.resize(1 + 2 + 4, 0);
            stream.read_exact(buf).await?;
            ClientMessage::KeyEvent
        }
        Ok(5) => {
            // PointerEvent
            buf.resize(1 + 2 + 2, 0);
            stream.read_exact(buf).await?;
            ClientMessage::PointerEvent
        }
        Ok(6) => {
            // ClientCutText
            buf.resize(3, 0);
            stream.read_exact(buf).await?; // drop padding
            let len = stream.read_u32().await?;
            buf.resize(len.try_into()?, 0);
            stream.read_exact(buf).await?;
            ClientMessage::ClientCutText
        }
        Ok(n) => bail!("Unknown client message: {}", n),
    };
    Ok(Some(msg))
}

pub(crate) async fn write_frame(
    stream: &mut TcpStream,
    rectangles: &[FrameRectangle],
) -> anyhow::Result<()> {
    // 7.6.1. FramebufferUpdate
    stream.write_u16(0).await?; // message-type + padding
    stream.write_u16(rectangles.len().try_into()?).await?;

    for rect in rectangles {
        stream.write_u16(rect.position.0).await?;
        stream.write_u16(rect.position.1).await?;
        stream.write_u16(rect.size.0).await?;
        stream.write_u16(rect.size.1).await?;
        stream.write_i32(rect.encoding.into()).await?;
        if rect.encoding == Encoding::Zrle {
            // 7.7.6. ZRLE
            stream.write_u32(rect.buf.len().try_into()?).await?;
        }
        stream.write_all(&rect.buf).await?;
    }
    Ok(())
}

impl PixelFormat {
    fn encode(&self) -> [u8; 16] {
        let mut bytes = [0u8; 16];
        let mut writer = &mut bytes[..];

        writer.write_u8(self.bits_per_pixel).unwrap();
        writer.write_u8(self.depth).unwrap();
        writer.write_u8(self.big_endian_flag.into()).unwrap();
        writer.write_u8(self.true_color_flag.into()).unwrap();

        writer.write_u16::<BE>(self.red_max).unwrap();
        writer.write_u16::<BE>(self.green_max).unwrap();
        writer.write_u16::<BE>(self.blue_max).unwrap();

        writer.write_u8(self.red_shift).unwrap();
        writer.write_u8(self.green_shift).unwrap();
        writer.write_u8(self.blue_shift).unwrap();

        // 3-byte trailing padding
        bytes
    }
}
