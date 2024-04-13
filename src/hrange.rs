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

use anyhow::Result;
use futures::stream::TryStreamExt;
use http::Uri;
use http_body_util::BodyStream;
use hyper::{
    client::conn::http1::{
        // Connection,
        SendRequest,
    },
    Request,
};
use hyper_util::rt::TokioIo;
use tokio::{io::AsyncRead, net::TcpStream};
use tokio_util::io::StreamReader;

///
#[derive(Debug, Clone)]
pub struct HttpFile {
    len: usize,
    uri: Uri,
    host: String,
}

///
async fn dial(uri: Uri) -> Result<(String, SendRequest<String>)> {
    let host = uri.host().ok_or(anyhow::anyhow!("no host"))?;

    let stream = TcpStream::connect(format!("{}:80", host)).await?;
    let io = TokioIo::new(stream);

    let (request_sender, connection) = hyper::client::conn::http1::handshake(io).await?;

    tokio::task::spawn(async move {
        if let Err(err) = connection.await {
            println!("Connection failed: {:?}", err);
        }
    });

    Ok((host.to_owned(), request_sender))
}

impl HttpFile {
    /// connect
    pub async fn connect(uri: &str) -> Result<Self> {
        let uri = uri.parse::<Uri>()?;
        let (host, mut request_sender) = dial(uri.clone()).await?;

        let req = Request::head(uri.path())
            .header("host", host.clone())
            .body("".to_owned())?;

        let res = request_sender.send_request(req).await?;

        let can_range = res
            .headers()
            .get("accept-ranges")
            .map(|v| v != "none")
            .unwrap_or(false);

        let len: usize = res
            .headers()
            .get("content-length")
            .map(|v| v.to_str().unwrap())
            .unwrap_or("0")
            .parse()
            .unwrap_or(0);

        if !can_range {
            anyhow::bail!("endpoint can't Range");
        }

        Ok(Self {
            len,
            uri: uri.clone(),
            host: host.to_owned(),
        })
    }

    /// return an AsyncRead at the specific offset to EOF
    pub async fn reader_at_to(&self, start: u64, len: u64) -> Result<Option<impl AsyncRead>> {
        if start >= (self.len as u64) {
            return Ok(None);
        }

        let (host, mut request_sender) = dial(self.uri.clone()).await?;

        let req = Request::get(self.uri.path())
            .header("range", format!("bytes={}-{}", start, start + len))
            .header("host", host)
            .body("".to_owned())?;

        request_sender.ready().await?;

        let res = request_sender.send_request(req).await?;
        let stream_of_bytes = BodyStream::new(res.into_body())
            .try_filter_map(|frame| async move { Ok(frame.into_data().ok()) })
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err));
        Ok(Some(Box::pin(StreamReader::new(stream_of_bytes))))
    }
}

// vim: foldmethod=marker
