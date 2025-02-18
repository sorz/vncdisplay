use anyhow::Context;
use clap::Parser;
use flate2::write::ZlibEncoder;
use log::{debug, info};
use rfp::FrameRectangle;
use tokio::net::{TcpListener, TcpStream};

mod cli;
mod rfp;
mod screen;

use screen::Screen;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let args = cli::Args::parse();
    env_logger::init();

    let screen = Screen::create(args.background, args.pointer)
        .context("Create screen from background picture")?;

    info!("Listen on {}", args.listen);
    let listener = TcpListener::bind(args.listen).await?;
    loop {
        let (stream, _) = listener.accept().await?;
        let peer = match stream.peer_addr() {
            Ok(peer) => peer,
            Err(err) => {
                info!("Connect error: {}", err);
                continue;
            }
        };
        debug!("Connected with {}", peer);

        let screen = screen.clone();
        let name = args.name.clone();
        tokio::spawn(async move {
            match handle_client(stream, screen, &name).await {
                Ok(()) => debug!("Disconnected with {}", peer),
                Err(err) => info!("Error on handle {}: {}", peer, err),
            }
        });
    }
}

async fn handle_client(
    mut stream: TcpStream,
    mut screen: Screen,
    name: &str,
) -> anyhow::Result<()> {
    let dims = screen.dimensions;
    rfp::handshake(&mut stream, dims, name)
        .await
        .context("RFP handshaking with client")?;
    let mut zlib: Option<ZlibEncoder<Vec<u8>>> = None;
    let mut pointer_supported = false;
    let mut buf = vec![0u8; 0];
    while let Some(msg) = rfp::read_message(&mut stream, &mut buf).await? {
        match msg {
            rfp::ClientMessage::SetPixelFormat(format) => {
                debug!("Client set pixel format: {:?}", format);
                screen
                    .set_pixel_format(format)
                    .context("Unsupported pixel format")?;
            }
            rfp::ClientMessage::SetEncodings(encodings) => {
                debug!("Client set encodings: {:?}", encodings);
                if encodings.contains(&rfp::Encoding::Zrle) {
                    let encoder = ZlibEncoder::new(Vec::new(), Default::default());
                    zlib = Some(encoder);
                }
                if encodings.contains(&rfp::Encoding::Cursor) {
                    pointer_supported = true;
                }
            }
            rfp::ClientMessage::FramebufferUpdateRequest { incremental, .. } => {
                debug!("Receive client message: {:?}", msg);
                if incremental {
                    continue; // Our screen is immuable
                }
                let rect = if let Some(encoder) = zlib.as_mut() {
                    FrameRectangle::new_zrle_frame(screen.dimensions, screen.draw_zrle(encoder)?)
                } else {
                    FrameRectangle::new_raw_frame(screen.dimensions, screen.draw_raw()?)
                };
                if let Some(pointer) = screen.draw_cursor().take_if(|_| pointer_supported) {
                    let pointer = FrameRectangle::new_cursor(screen.pointer_size(), pointer);
                    rfp::write_frame(&mut stream, &[rect, pointer]).await?;
                } else {
                    rfp::write_frame(&mut stream, &[rect]).await?;
                }
            }
            rfp::ClientMessage::KeyEvent
            | rfp::ClientMessage::PointerEvent
            | rfp::ClientMessage::ClientCutText => continue, // ignore
        }
    }
    Ok(())
}
