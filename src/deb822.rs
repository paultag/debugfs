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

use std::collections::HashMap;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, Error as IoError};

#[derive(Debug)]
pub enum Error {
    Malformed,
    Io(IoError),
}

impl From<IoError> for Error {
    fn from(ioe: IoError) -> Self {
        Self::Io(ioe)
    }
}

///
pub async fn next<T>(mut b: T) -> Result<Option<HashMap<String, String>>, Error>
where
    T: Unpin,
    T: AsyncBufRead,
{
    let mut ret = HashMap::new();
    loop {
        let mut line = String::new();
        let n = b.read_line(&mut line).await?;
        if n == 0 {
            if ret.is_empty() {
                return Ok(None);
            }
            break;
        }

        let line = line.trim();
        if line == "" {
            break;
        }
        let (key, value) = match line.split_once(":") {
            None => return Err(Error::Malformed),
            Some(v) => v,
        };
        ret.insert(key.trim().to_owned(), value.trim().to_owned());
    }

    Ok(Some(ret))
}

#[cfg(test)]
mod test {
    use super::next;
    use std::io::Cursor;

    #[tokio::test]
    async fn split_default() {
        let release = "Package: zziplib-bin-dbgsym
Build-Ids: 204d62991035324322317de6f71f494c06a10d37 23c08beddf41e0098035f3c34274450ccc0a9f21 24853edbb338b1ce4c09b45281a64d28d7d0a99a 34222ab9d2c0784113e2c991c43c9edda92fd84c 5450729f198b53118807b4ff47160c47929d861d 558c758a6c699700f7bde7672c62312633894838 6653faaf15da53e47d66fffad6cadaa55ab70b2a 8eaa681a70ef64e31de5e682a27c17edeeaa0144 ad3e49a7ea5869ec5f24b928511986ce47906548 f9fd0e00a95264500fef36579447493a3f17e374
Filename: pool/main/z/zziplib/zziplib-bin-dbgsym_0.13.72+dfsg.1-1.2_amd64.deb

Package: zzuf-dbgsym
Build-Ids: 1c54e04fcf760c428d0afa79a33ffb8e068d35d5 49a0ba466e7cea361ccb59d054ba9986a1ab7824 9f6d2126e25fb242c203472a2a6d32157f33af70
Filename: pool/main/z/zzuf/zzuf-dbgsym_0.15-2+b4_amd64.deb
";

        let mut cur = Cursor::new(release);
        let zzip = next(&mut cur).await.unwrap().unwrap();
        assert_eq!("zziplib-bin-dbgsym", zzip["Package"]);

        let zuf = next(&mut cur).await.unwrap().unwrap();
        assert_eq!("zzuf-dbgsym", zuf["Package"]);

        assert!(next(&mut cur).await.unwrap().is_none());
    }
}

// vim: foldmethod=marker
