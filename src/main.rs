use std::sync::Arc;

use anyhow::{bail, Context};
use clap::Parser;
use image::ImageReader;
use log::{debug, info};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

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
        tokio::spawn(async move {
            match handle_client(stream, screen).await {
                Ok(()) => debug!("Disconnected with {}", peer),
                Err(err) => info!("Error on handle {}: {}", peer, err),
            }
        });
    }
}

async fn handle_client(mut stream: TcpStream, screen: Arc<Screen>) -> anyhow::Result<()> {
    rfp::handshake(&mut stream, screen.dimensions)
        .await
        .context("RFP handshaking with client")?;
    Ok(())
}
