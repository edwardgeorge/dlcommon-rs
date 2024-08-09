use std::error::Error;

use clap::ValueEnum;
use reqwest::Url;
use reqwest_cookie_store::{CookieStore, CookieStoreMutex, RawCookie};
#[cfg(target_os = "macos")]
use rookie::safari;
use rookie::{brave, chrome, edge, enums::Cookie, firefox, opera};
use strum::{Display, EnumString};
use time::OffsetDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString, Display, Default, ValueEnum)]
#[strum(serialize_all = "lowercase")]
pub enum Browser {
    Brave,
    Chrome,
    Edge,
    #[default]
    Firefox,
    Opera,
    #[cfg(target_os = "macos")]
    Safari,
}

impl Browser {
    // pub fn as_str(&self) -> &'static str {
    //     match self {
    //         Self::Firefox => "firefox",
    //     }
    // }
    fn get_cookies(&self, domains: Option<Vec<String>>) -> Result<Vec<Cookie>, Box<dyn Error>> {
        Ok(match self {
            Self::Brave => brave(domains)?,
            Self::Edge => edge(domains)?,
            Self::Firefox => firefox(domains)?,
            Self::Chrome => chrome(domains)?,
            Self::Opera => opera(domains)?,
            #[cfg(target_os = "macos")]
            Self::Safari => safari(domains)?,
        })
    }
}

pub fn get_cookies(
    browser: Browser,
    domains: Option<Vec<String>>,
) -> Result<CookieStoreMutex, Box<dyn Error>> {
    let mut cs = CookieStore::new(None);
    for c in browser.get_cookies(domains)? {
        cs.insert_raw(
            &RawCookie::build((&c.name, &c.value))
                .domain(&c.domain)
                .secure(c.secure)
                .http_only(c.http_only)
                .expires(
                    c.expires
                        .map(|i| OffsetDateTime::from_unix_timestamp(i as i64).unwrap()),
                )
                .build(),
            &Url::parse(&format!(
                "https://{}{}",
                c.domain.trim_start_matches('.'),
                &c.path
            ))?,
        )
        .map_err(|e| format!("Got error on {c:?}: {e}"))?;
    }
    Ok(CookieStoreMutex::new(cs))
}
