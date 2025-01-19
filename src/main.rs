use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use flate2::write::ZlibEncoder;
use log::{debug, info};
use tokio::net::{TcpListener, TcpStream};

mod cli;
mod rfp;
mod screen;

use screen::Screen;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let args = cli::Args::parse();
    env_logger::init();

    let screen =
        Screen::create(args.background).context("Create screen from background picture")?;
    let screen = Arc::new(screen);

    info!("Listen on {}", args.listen);
    let listener = TcpListener::bind(args.listen).await?;
    loop {
        let (stream, _) = listener.accept().await?;
        let peer = stream.peer_addr()?;
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
    screen: Arc<Screen>,
    name: &str,
) -> anyhow::Result<()> {
    let dims = screen.dimensions;
    rfp::handshake(&mut stream, dims, name)
        .await
        .context("RFP handshaking with client")?;
    let mut zlib: Option<ZlibEncoder<Vec<u8>>> = None;
    let mut buf = vec![0u8; 0];
    while let Some(msg) = rfp::read_message(&mut stream, &mut buf).await? {
        match msg {
            rfp::ClientMessage::SetPixelFormat => {
                debug!("Receive client message: {:?}", msg);
            }
            rfp::ClientMessage::SetEncodings(encodings) => {
                debug!("Client set encodings: {:?}", encodings);
                if encodings.contains(&rfp::Encoding::Zrle) {
                    let encoder = ZlibEncoder::new(Vec::new(), Default::default());
                    zlib = Some(encoder);
                }
            }
            rfp::ClientMessage::FramebufferUpdateRequest { incremental, .. } => {
                debug!("Receive client message: {:?}", msg);
                if incremental {
                    continue; // TODO: send empty update instead of ignoring
                }
                if let Some(encoder) = zlib.as_mut() {
                    let frame = screen.draw_zrle(encoder)?;

                    rfp::write_frame(
                        &mut stream,
                        (0, 0),
                        screen.dimensions,
                        rfp::Encoding::Zrle,
                        &frame,
                    )
                    .await?;
                } else {
                    let frame = screen.draw_raw();
                    rfp::write_frame(
                        &mut stream,
                        (0, 0),
                        screen.dimensions,
                        rfp::Encoding::Raw,
                        &frame,
                    )
                    .await?;
                }
            }
            rfp::ClientMessage::KeyEvent
            | rfp::ClientMessage::PointerEvent
            | rfp::ClientMessage::ClientCutText => continue, // ignore
        }
    }
    Ok(())
}
