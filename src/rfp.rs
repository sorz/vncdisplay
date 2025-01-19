use anyhow::{bail, Context};
use log::debug;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum RfpVersion {
    V3_3,
    V3_7,
    V3_8,
}

static SECURITY_TYPE_NO_AUTHENTICATION: u8 = 0;
static SECURITY_RESULT_OK: u32 = 0;
static SECURITY_RESULT_FAILED: u32 = 1;

static ERROR_REASON_PROTOCOL_VERSION_UNSUPPORTED: &str = "Unsupported protocol version";
static ERROR_REASON_SECURITY_TYPE_UNSUPPORTED: &str = "Unsupported security type";

/// Handshake with client.
/// From TCP connection established to initialization messages exchanged.
pub(crate) async fn handshake(stream: &mut TcpStream) -> anyhow::Result<()> {
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
    // TODO: impl ServerInit

    Ok(())
}
