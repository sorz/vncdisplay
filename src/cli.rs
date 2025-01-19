use std::{net::SocketAddr, path::PathBuf};

use clap::Parser;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub(crate) struct Args {
    /// TCP address to listen
    #[arg(short, long, default_value = "[::]:5900")]
    pub(crate) listen: SocketAddr,

    /// Background picture
    #[arg(short, long)]
    pub(crate) background: PathBuf,

    /// Pointer picture
    #[arg(short, long)]
    pub(crate) pointer: Option<PathBuf>,

    /// Desktop name
    #[arg(short, long, default_value = "VNC Display")]
    pub(crate) name: String,


}
