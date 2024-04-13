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

use super::HttpFile;
use anyhow::Result;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{duplex, AsyncRead, AsyncReadExt, AsyncWriteExt, DuplexStream, ReadBuf};

use xz2::stream::{Action, Status};

///
const MAGIC: [u8; 8] = *b"!<arch>\n";

///
const TRAILER: [u8; 2] = [0x60, 0x0A];

pub struct Deb {
    file: HttpFile,
    offset: u64,
}

trait AsyncReadSend = AsyncRead + Unpin + Send + 'static;

///
pub struct DebEntry {
    header: Header,
    body: Pin<Box<dyn AsyncReadSend>>,
}

impl DebEntry {
    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn into_body(self) -> Pin<Box<dyn AsyncReadSend>> {
        self.body
    }
}

///
#[derive(Debug, Clone)]
pub struct Header {
    pub identifier: String,
    pub size: u64,
    pub timestamp: u64,
    pub owner: u64,
    pub group: u64,
    pub mode: u64,
}

#[derive(Debug, Clone)]
#[repr(packed)]
struct RawHeader {
    identifier: [u8; 16],
    timestamp: [u8; 12],
    owner: [u8; 6],
    group: [u8; 6],
    mode: [u8; 8],
    size: [u8; 10],
    trailer: [u8; 2],
}

type JoinSet = tokio::task::JoinSet<Result<()>>;

#[pin_project::pin_project]
pub struct Decompress {
    join_set: JoinSet,

    #[pin]
    pipe: DuplexStream,
}

impl Decompress {
    pub async fn new<T: AsyncReadSend>(mut body: T) -> Result<Self> {
        let mut decoder = xz2::stream::Stream::new_stream_decoder(u64::MAX, 0).unwrap();
        let (pipe, mut pipe1) = duplex(1024 * 32);
        let mut join_set = JoinSet::new();

        join_set.build_task().name("").spawn(async move {
            loop {
                let mut compressed = vec![0u8; 1024 * 32];
                let mut output: Vec<u8> = Vec::with_capacity(1024 * 128);

                let n = body.read(&mut compressed).await?;
                let compressed = &compressed[..n];
                if n == 0 {
                    break;
                }
                let n = n as u64;
                let end = decoder.total_in() + n;
                while decoder.total_in() < end {
                    let start = compressed.len() - (end - decoder.total_in()) as usize;
                    output.clear();
                    decoder.process_vec(&compressed[start..], &mut output, Action::Run)?;
                    pipe1.write_all(&output).await?;
                }
            }

            Ok(())
        })?;

        Ok(Decompress { pipe, join_set })
    }
}

impl AsyncRead for Decompress {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<tokio::io::Result<()>> {
        let this = self.project();
        this.pipe.poll_read(cx, buf)
    }
}

impl Deb {
    ///
    pub async fn open(host: &str) -> Result<Deb> {
        let file = HttpFile::connect(host).await?;

        let mut prefix = [0u8; 8];
        file.reader_at_to(0, 8)
            .await?
            .ok_or(anyhow::anyhow!("file is empty"))?
            .read_exact(&mut prefix)
            .await?;

        if prefix != MAGIC {
            anyhow::bail!("wrong file magic; is this an .ar file?");
        }

        Ok(Deb { file, offset: 8 })
    }

    ///
    pub async fn next(&mut self) -> Result<Option<DebEntry>> {
        let mut header = [0u8; 60];
        let mut reader = match self.file.reader_at_to(self.offset, 60).await? {
            None => return Ok(None),
            Some(v) => v,
        };

        reader.read_exact(&mut header).await?;

        let header = unsafe { std::mem::transmute::<[u8; 60], RawHeader>(header) };

        if header.trailer != TRAILER {
            anyhow::bail!("trailer is wrong; file corrupted?");
        }

        let raw2str = |raw| Ok::<&str, anyhow::Error>(std::str::from_utf8(raw)?.trim());

        let identifier = raw2str(&header.identifier)?.to_owned();
        let size: u64 = raw2str(&header.size)?.parse()?;
        let timestamp: u64 = raw2str(&header.timestamp)?.parse()?;
        let owner: u64 = raw2str(&header.owner)?.parse()?;
        let group: u64 = raw2str(&header.group)?.parse()?;
        let mode: u64 = raw2str(&header.mode)?.parse()?;

        self.offset += 60;
        let mut reader = match self.file.reader_at_to(self.offset, size).await? {
            None => return Ok(None),
            Some(v) => v,
        };
        self.offset += size;

        Ok(Some(DebEntry {
            body: Box::pin(reader),
            header: Header {
                identifier,
                size,
                timestamp,
                owner,
                group,
                mode,
            },
        }))
    }
}

// vim: foldmethod=marker
