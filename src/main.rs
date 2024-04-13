// {{{ Copyright (c) Paul R. Tagliamonte <paultag@gmail.com>, 2023-2024
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE. }}}

#![feature(trait_alias)]

use arigato::server::AsyncServer;
use std::str::FromStr;
use tokio::io::AsyncReadExt;
use tokio_stream::StreamExt;
use tokio_tar::Archive;
use tracing_subscriber::{fmt::format::FmtSpan, FmtSubscriber};
use xz2::stream::Action;

mod ar;
mod deb822;
mod debugfs;
mod hrange;

use ar::{Deb, Decompress};
use debugfs::Debug;
use hrange::HttpFile;
use xz2::{read::XzDecoder, stream::Status};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let log_level = "info";

    let subscriber = FmtSubscriber::builder()
        .with_writer(std::io::stderr)
        .with_max_level(tracing::Level::from_str(log_level).unwrap())
        .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    let srv = AsyncServer::builder()
        .with_tcp_listen_address("127.0.0.1:5640")
        .with_filesystem(
            "unstable",
            Debug::new(
                "http://archive.adref/debian-debug/",
                "unstable-debug",
                "main",
            ),
        )
        .build()
        .await
        .unwrap();
    srv.serve().await.unwrap();

    Ok(())
}

// vim: foldmethod=marker
