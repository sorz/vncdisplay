use std::{io::{self, Write}, u32};

use anyhow::{bail, Context};
use byteorder_lite::{WriteBytesExt, BE};
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
struct PixelFormat {
    bits_per_pixel: u8,
    depth: u8,
    big_endian_flag: bool,
    true_color_flag: bool,
    red_max: u16,
    green_max: u16,
    blue_max: u16,
    red_shift: u8,
    green_shift: u8,
    blue_shift: u8,
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
    stream.write_all(&name.as_bytes()[..name_len as usize]).await?;
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