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

use super::{deb822, Deb, Decompress};
use arigato::{
    raw::{Dehydrate, FileType, IoDirection, OpenMode, Qid, Stat},
    server::{File as FileTrait, FileError, FileResult, Filesystem, OpenFile as OpenFileTrait},
};
use futures::TryFutureExt;
use std::{
    collections::HashMap,
    io::{Cursor, Read, Seek, SeekFrom},
    sync::Arc,
};
use tokio::io::AsyncReadExt;
use tokio_stream::StreamExt;
use tokio_tar::{Archive, Entry};
use xz2::read::XzDecoder;
use xz2::stream::Action;

// ///
// type JoinSet = tokio::task::JoinSet<()>;

///
pub(crate) struct Debug {
    archive_root: String,
    // suite: String,
    // component: String,
    packages: String,
}

impl Debug {
    ///
    pub fn new(archive_root: &str, suite: &str, component: &str) -> Self {
        Debug {
            archive_root: archive_root.to_owned(),
            // suite: suite.to_owned(),
            // component: component.to_owned(),
            packages: format!("{archive_root}/dists/{suite}/{component}/binary-amd64/Packages.xz"),
        }
    }
}

impl Filesystem for Debug {
    type File = File;

    async fn attach(&self, _: &str, _: &str, _: u32) -> FileResult<File> {
        tracing::info!("requesting {}", &self.packages);
        let client = reqwest::Client::new();
        let response = client
            .get(&self.packages)
            .send()
            .await
            .map_err(|_| FileError(121, "EREMOTEIO".to_owned()))?;

        if response.status() != 200 {
            return Err(FileError(121, "EREMOTEIO".to_owned()));
        }

        let decompressor = XzDecoder::new(Cursor::new(
            response
                .bytes()
                .await
                .map_err(|_| FileError(121, "EREMOTEIO".to_owned()))?,
        ));
        let response_bytes: std::io::Result<Vec<u8>> = decompressor.bytes().collect();
        let response_bytes = response_bytes.map_err(|_| FileError(121, "EREMOTEIO".to_owned()))?;
        let mut body = Cursor::new(response_bytes);

        let mut entries = HashMap::<String, (String, Vec<File>)>::new();
        loop {
            let headers = match deb822::next(&mut body)
                .await
                .map_err(|_| FileError(121, "EREMOTEIO".to_owned()))?
            {
                None => {
                    break;
                }
                Some(v) => v,
            };

            let build_ids = headers.get("Build-Ids");
            if let None = build_ids {
                // malformed
                continue;
            }

            let path = match headers.get("Filename") {
                None => {
                    // malformed
                    continue;
                }
                Some(v) => v,
            };

            for build_id in build_ids.unwrap().split(" ") {
                let dir_name = build_id[..2].to_owned();
                let (_, dir_entries) = entries
                    .entry(dir_name.clone())
                    .or_insert((dir_name.clone(), vec![]));
                dir_entries.push(File::DebugHeader(DebugHeader {
                    fspath: format!("{}/{}.debug", dir_name, &build_id[2..]),
                    build_id: build_id.to_owned(),
                    name: format!("{}.debug", &build_id[2..]),
                    pool: format!("{}/{}", self.archive_root, path),
                }));
            }
        }

        // tokio spawn on a joinset

        Ok(File::Root(Root {
            // join_set: Arc::new(JoinSet::new()),
            directory: Arc::new(Box::new(Directory {
                name: "/".to_owned(),
                entries: Arc::new(
                    entries
                        .into_iter()
                        .map(|(_, (name, entries))| {
                            File::Directory(Directory {
                                name,
                                entries: Arc::new(entries),
                            })
                        })
                        .collect(),
                ),
            })),
        }))
    }
}

///
#[derive(Debug, Clone)]
pub(crate) struct Directory {
    name: String,
    entries: Arc<Vec<File>>,
}

impl Directory {
    ///
    async fn open_dir(&self, om: OpenMode) -> FileResult<OpenFile> {
        match om.direction() {
            IoDirection::Read => {}
            _ => return Err(FileError(1, "EPERM".to_owned())),
        }

        let mut ent = Cursor::new(vec![]);
        for dirent in self.entries.iter() {
            let stat = dirent.stat().await?;
            match stat.dehydrate(&mut ent) {
                Ok(_) => {}
                Err(_) => return Err(FileError(22, "EINVAL".to_owned())),
            }
        }
        Ok(OpenFile::Cursor(ent))
    }
}

///
#[derive(Debug, Clone)]
pub(crate) struct Root {
    directory: Arc<Box<Directory>>,
    // join_set: Arc<JoinSet>,
}

///
#[derive(Debug, Clone)]
pub(crate) struct DebugHeader {
    name: String,
    build_id: String,
    pool: String,
    fspath: String,
}

///
#[derive(Debug, Clone)]
pub(crate) enum File {
    ///
    Root(Root),

    ///
    Directory(Directory),

    ///
    DebugHeader(DebugHeader),
}

pub(crate) enum OpenFile {
    ///
    Cursor(Cursor<Vec<u8>>),

    ///
    DebEntry(DebEntry),
}

struct DebEntry {
    offset: u64,
    file: Entry<Archive<Decompress>>,
}

impl DebEntry {
    async fn read_at(&mut self, buf: &mut [u8], off: u64) -> FileResult<u64> {
        if off != self.offset {
            return Err(FileError(29, "ESPIPE".to_owned()));
        }
        let n = self
            .file
            .read(buf)
            .await
            .map_err(|_| FileError(5, "EIO".to_owned()))? as u64;
        self.offset += n;
        Ok(n)
    }
}

impl DebugHeader {
    async fn open_file(&self, om: OpenMode) -> FileResult<OpenFile> {
        match om.direction() {
            IoDirection::Read => {}
            _ => return Err(FileError(1, "EPERM".to_owned())),
        }

        tracing::debug!("opening deb: {}", self.pool);
        let mut deb = Deb::open(&self.pool)
            .await
            .map_err(|_| FileError(5, "EIO".to_owned()))?;

        loop {
            let entry = match deb
                .next()
                .await
                .map_err(|_| FileError(5, "EIO".to_owned()))?
            {
                None => return Err(FileError(5, "EIO".to_owned())),
                Some(v) => v,
            };
            tracing::debug!("loaded entry {:?}", entry.header());

            if entry.header().identifier == "data.tar.xz" {
                let mut decoder = xz2::stream::Stream::new_stream_decoder(u64::MAX, 0).unwrap();
                let mut body = entry.into_body();
                let mut ar = Archive::new(
                    Decompress::new(body)
                        .await
                        .map_err(|_| FileError(5, "EIO".to_owned()))?,
                );
                tracing::debug!("stream decompressing");

                let mut entries = ar.entries().map_err(|_| FileError(5, "EIO".to_owned()))?;
                while let Some(file) = entries.next().await {
                    let mut file = file.map_err(|_| FileError(5, "EIO".to_owned()))?;
                    tracing::debug!("found file {:?}", file.path());

                    if file
                        .path()
                        .map_err(|_| FileError(5, "EIO".to_owned()))?
                        .as_os_str()
                        .to_str()
                        .unwrap()
                        == format!("./usr/lib/debug/.build-id/{}", self.fspath)
                    {
                        let mut header = Vec::new();
                        file.read_to_end(&mut header)
                            .map_err(|_| FileError(5, "EIO".to_owned()))
                            .await?;

                        return Ok(OpenFile::Cursor(Cursor::new(header)));

                        // return Ok(OpenFile::DebEntry(DebEntry { offset: 0, file }));
                    }
                }
            }
        }
    }
}

impl File {
    fn name(&self) -> &str {
        match self {
            Self::Root(_) => "/",
            Self::Directory(dir) => &dir.name,
            Self::DebugHeader(dbg) => &dbg.name,
        }
    }

    async fn walk_to(&self, path: &str) -> FileResult<Self> {
        match self {
            Self::Root(root) => {
                for entry in root.directory.entries.iter() {
                    if entry.name() == path {
                        return Ok(entry.clone());
                    }
                }
            }
            Self::Directory(dir) => {
                for entry in dir.entries.iter() {
                    if entry.name() == path {
                        return Ok(entry.clone());
                    }
                }
            }
            _ => {}
        };
        Err(FileError(2, "ENOENT".to_owned()))
    }
}

impl FileTrait for File {
    type OpenFile = OpenFile;

    async fn stat(&self) -> FileResult<Stat> {
        let qid = self.qid();

        let sb = Stat::builder(self.name(), qid)
            .with_nuid(0)
            .with_ngid(0)
            .with_nmuid(0)
            .with_size(1_000_000_000);

        let sb = match self {
            Self::Root(_) => sb.with_mode(0o555),
            Self::Directory(_) => sb.with_mode(0o555),
            Self::DebugHeader(_) => sb.with_mode(0o444),
        };

        Ok(sb.build())
    }

    async fn wstat(&mut self, _: &Stat) -> FileResult<()> {
        Err(FileError(1, "EPERM".to_owned()))
    }

    async fn walk(&self, path: &[&str]) -> FileResult<(Option<Self>, Vec<Self>)> {
        if path.is_empty() {
            return Ok((Some(self.clone()), vec![]));
        }

        let mut my_path = self.clone();
        let mut walked_path = vec![];
        for part in path {
            my_path = match self.walk_to(part).await {
                Ok(v) => {
                    walked_path.push(my_path);
                    v
                }
                Err(_) => return Ok((None, walked_path)),
            };
        }

        Ok((Some(my_path), walked_path))
    }

    async fn unlink(&mut self) -> FileResult<()> {
        Err(FileError(1, "EPERM".to_owned()))
    }

    async fn create(
        &mut self,
        _: &str,
        _: u16,
        _: arigato::raw::FileType,
        _: OpenMode,
        _: &str,
    ) -> FileResult<Self> {
        Err(FileError(1, "EPERM".to_owned()))
    }

    async fn open(&mut self, om: OpenMode) -> FileResult<OpenFile> {
        match self {
            Self::Directory(dir) => dir.open_dir(om).await,
            Self::Root(root) => root.directory.open_dir(om).await,
            Self::DebugHeader(dh) => dh.open_file(om).await,
        }
    }

    fn qid(&self) -> Qid {
        match self {
            Self::Root(_) => Qid::new(FileType::Dir, 0x01, 0x01),
            Self::Directory(dir) => {
                let id = u64::from_str_radix(&dir.name, 16).unwrap();
                Qid::new(FileType::Dir, 0x01, id)
            }
            Self::DebugHeader(dh) => {
                let id = u64::from_str_radix(&dh.build_id[..16], 16).unwrap();
                Qid::new(FileType::File, 0x01, id)
            }
        }
    }
}

impl OpenFileTrait for OpenFile {
    fn iounit(&self) -> u32 {
        0
    }

    async fn read_at(&mut self, buf: &mut [u8], off: u64) -> FileResult<u32> {
        match self {
            Self::DebEntry(file) => Ok(file.read_at(buf, off).await?.try_into().unwrap()),
            Self::Cursor(cur) => {
                cur.seek(SeekFrom::Start(off))?;
                Ok(std::io::Read::read(cur, buf)?.try_into().unwrap())
            }
        }
    }

    async fn write_at(&mut self, _buf: &mut [u8], _off: u64) -> FileResult<u32> {
        Err(FileError(1, "EPERM".to_owned()))
    }
}

// vim: foldmethod=marker
