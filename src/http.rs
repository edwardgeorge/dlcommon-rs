use std::{error::Error, path::Path, str::from_utf8, sync::Arc};

use futures_util::StreamExt as _;
use mailparse::DispositionType;
use percent_encoding::percent_decode_str;
use reqwest::Client;
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

pub async fn download_file<F>(
    client: &Client,
    url: &str,
    target: &Path,
    progress_cb: Option<F>,
) -> Result<(String, Outcome), Box<dyn Error>>
where
    F: Fn(u64, u64),
{
    let r = client.get(url).send().await?.error_for_status()?;
    let len: u64 = r
        .headers()
        .get("Content-length")
        .ok_or("No content-length header")?
        .to_str()?
        .parse()?;
    let disposition_header = r
        .headers()
        .get("Content-disposition")
        .ok_or("No content-disposition header")?;
    let disposition = disposition_header.to_str().or_else(|_| {
        from_utf8(disposition_header.as_bytes())
            .map_err(|e| format!("Could not decode disposition header from UTF8: {e}"))
    })?;
    let filename = crate::http::filename_from_disposition(disposition)?;
    let target_file = target.join(&filename);
    let mut outcome = Outcome::Download(len);
    if !target.exists() {
        create_dir_all(target).await?;
    } else if target_file.exists() {
        if target_file.is_file() {
            let meta = target_file.metadata()?;
            if meta.len() != len {
                log::info!(
                    "File '{}' is not the expected size... overwriting...",
                    target_file.display()
                );
                outcome = Outcome::Redownload(len);
            } else {
                return Ok((filename, Outcome::Existing));
            }
        } else {
            return Err(format!(
                "File '{}' already exists and is not a regular file!",
                target_file.display()
            )
            .into());
        }
    }
    let mut f = crate::file::AtomicFile::open(&target_file).await?;
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
