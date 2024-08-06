use std::{borrow::Cow, error::Error, path::Path, str::from_utf8, sync::Arc};

use derive_builder::Builder;
use futures_util::StreamExt as _;
use mailparse::DispositionType;
use percent_encoding::percent_decode_str;
use reqwest::{Client, Method, Response};
use reqwest_cookie_store::CookieStoreMutex;
use tokio::fs::create_dir_all;

pub fn get_client(cs: Option<Arc<CookieStoreMutex>>) -> Result<Client, Box<dyn Error>> {
    let mut cb = Client::builder()
        .user_agent(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:122.0) Gecko/20100101 Firefox/122.0",
        )
        .cookie_store(true)
        .gzip(true);
    cb = match cs {
        Some(v) => cb.cookie_provider(v),
        None => cb.cookie_store(true),
    };
    Ok(cb.build()?)
}

pub fn filename_from_disposition(cd: &str) -> Result<String, Box<dyn Error>> {
    let x = mailparse::parse_content_disposition(cd);
    if let DispositionType::Attachment = x.disposition {
        Ok(x.params
            .get("filename*")
            .and_then(|i| i.strip_prefix("UTF-8''"))
            .and_then(|i| percent_decode_str(i).decode_utf8().ok())
            .or_else(|| {
                x.params
                    .get("filename")
                    .and_then(|i| percent_decode_str(i).decode_utf8().ok())
            })
            .ok_or_else(|| {
                format!("Could not parse a filename from the content-disposition header '{cd}'")
            })?
            .to_string())
    } else {
        Err(format!(
            "Content-disposition is expected to be an attachment with filename param. got '{cd}'"
        )
        .into())
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Outcome {
    Download(u64),
    Redownload(u64),
    Existing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverwriteBehaviour {
    Always,
    CheckLength,
    #[default]
    Never,
    Fail,
}

impl OverwriteBehaviour {
    #[inline]
    fn conditional(&self) -> bool {
        matches!(self, Self::CheckLength)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UsagePref {
    Require,
    Prefer, // if available
    #[default]
    Reject,
}

impl UsagePref {
    #[inline]
    fn bool(&self) -> bool {
        matches!(self, Self::Require | Self::Prefer)
    }
    #[inline]
    fn strict(&self) -> bool {
        matches!(self, Self::Require)
    }
}

#[derive(Debug, Clone, Builder)]
pub struct FileDownload<'a> {
    client: &'a Client,
    url: &'a str,
    target: &'a Path,
    #[builder(default)]
    preflight_head: bool,
    #[builder(default)]
    overwrite: OverwriteBehaviour,
    #[builder(default)]
    filename_use_content_disposition: UsagePref,
    #[builder(default)]
    filename_use_final_url: UsagePref,
    #[builder(default, setter(into))]
    filename: Option<Cow<'a, str>>,
}

impl<'a> FileDownload<'a> {
    pub fn builder() -> FileDownloadBuilder<'a> {
        FileDownloadBuilder::default()
    }
    // pub fn with_preflight_head(&mut self, flag: bool) -> &mut Self {
    //     self.preflight_head = flag;
    //     self
    // }
    // pub fn with_overwrite(&mut self, behaviour: OverwriteBehaviour) -> &mut Self {
    //     self.overwrite = behaviour;
    //     self
    // }
    // pub fn filename_use_content_disposition(&mut self, flag: bool) -> &mut Self {
    //     self.filename_use_content_disposition = flag;
    //     self
    // }
    // pub fn filename_use_final_url(&mut self, flag: bool) -> &mut Self {
    //     self.filename_use_final_url = flag;
    //     self
    // }
    #[inline]
    fn expect_filename(&self) -> bool {
        self.filename_use_content_disposition.bool() || self.filename_use_final_url.bool()
    }
    #[inline]
    fn should_preflight(&self) -> bool {
        self.preflight_head && (self.expect_filename() || self.overwrite.conditional())
    }
    fn filename(&self, resp: &Response) -> Result<Option<String>, Box<dyn Error>> {
        if self.filename_use_content_disposition.bool() {
            if let Some(disposition_header) = resp.headers().get("Content-disposition") {
                let disposition = disposition_header.to_str().or_else(|_| {
                    from_utf8(disposition_header.as_bytes())
                        .map_err(|e| format!("Could not decode disposition header from UTF8: {e}"))
                })?;
                return Ok(Some(filename_from_disposition(disposition)?));
            } else if self.filename_use_content_disposition.strict() {
                return Err("No content-disposition header".into());
            }
        }
        if self.filename_use_final_url.bool() {
            unimplemented!()
        }
        if let Some(f) = &self.filename {
            return Ok(Some(f.to_string()));
        } else if self.expect_filename() {
            return Err("filename required but no default provided".into());
        }
        Ok(None)
    }
    pub async fn download<F>(
        &self,
        progress_cb: Option<F>,
    ) -> Result<(Cow<'a, Path>, Outcome), Box<dyn Error>>
    where
        F: Fn(u64, u64),
    {
        let preflight = self.should_preflight();
        let r = self
            .client
            .request(if preflight { Method::HEAD } else { Method::GET }, self.url)
            .send()
            .await?
            // TODO: fallback to GET if we get a 405 Method Not Allowed?
            .error_for_status()?;
        let len: u64 = r
            .headers()
            .get("Content-length")
            .ok_or("No content-length header")?
            .to_str()?
            .parse()?;
        let filename: Cow<'_, Path> = self.filename(&r)?.map_or_else(
            || Cow::Borrowed(self.target),
            |f| Cow::Owned(self.target.join(f)),
        );
        let outcome = if filename.exists() {
            if !filename.is_file() {
                return Err(format!(
                    "File exists and is not a regular file: '{}'",
                    filename.display()
                )
                .into());
            }
            match self.overwrite {
                OverwriteBehaviour::Never => return Ok((filename, Outcome::Existing)),
                OverwriteBehaviour::Fail => {
                    return Err(
                        format!("File '{}' already exists. failing!", filename.display()).into(),
                    )
                }
                OverwriteBehaviour::Always => (),
                OverwriteBehaviour::CheckLength => {
                    let meta = filename.metadata()?;
                    if meta.len() != len {
                        log::info!(
                            "File '{}' is not the expected size... overwriting...",
                            filename.display()
                        );
                    } else {
                        return Ok((filename, Outcome::Existing));
                    }
                }
            }
            Outcome::Redownload(len)
        } else {
            if let Some(parent) = filename.parent() {
                create_dir_all(parent).await?;
            }
            Outcome::Download(len)
        };
        let r = if preflight {
            self.client.get(self.url).send().await?.error_for_status()?
        } else {
            r
        };
        let mut f = crate::file::AtomicFile::open(&filename).await?;
        let mut bytestream = r.bytes_stream();
        let mut bytes = 0;
        if let Some(f) = progress_cb.as_ref() {
            f(len, 0);
        }
        while let Some(v) = bytestream.next().await {
            let b = v?;
            bytes += b.len();
            f.write_all(&b).await?;
            if let Some(f) = progress_cb.as_ref() {
                f(len, bytes as u64);
            }
        }
        f.commit().await?;
        Ok((filename, outcome))
    }
}

pub async fn download_file<F>(
    client: &Client,
    url: &str,
    target: &Path,
    progress_cb: Option<F>,
) -> Result<(String, Outcome), Box<dyn Error>>
where
    F: Fn(u64, u64),
{
    let (a, b) = FileDownloadBuilder::default()
        .client(client)
        .url(url)
        .target(target)
        .filename_use_content_disposition(UsagePref::Require)
        .build()?
        .download(progress_cb)
        .await?;
    Ok((a.display().to_string(), b))
}
