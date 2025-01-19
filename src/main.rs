use anyhow::{bail, Context};
use clap::Parser;
use log::{debug, info};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

mod cli;
mod rfp;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let args = cli::Args::parse();

    info!("Listen on {}", args.listen);
    let listener = TcpListener::bind(args.listen).await?;
    loop {
        let (stream, _) = listener.accept().await?;
        let peer = stream.peer_addr()?;
        tokio::spawn(async move {
            match handle_client(stream).await {
                Ok(()) => debug!("Disconnected with {}", peer),
                Err(err) => info!("Error on handle {}: {}", peer, err),
            }
        });
    }
}

async fn handle_client(mut stream: TcpStream) -> anyhow::Result<()> {
    rfp::handshake(&mut stream)
        .await
        .context("RFP handshaking with client")?;
    Ok(())
}
