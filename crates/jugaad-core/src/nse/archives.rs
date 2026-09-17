use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use reqwest::{Client, StatusCode};
use zip::ZipArchive;

use crate::error::{Error, Result};

const BASE_URL: &str = "https://nsearchives.nseindia.com";

// NSE blocks requests that don't look like they came from a browser.
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
    (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

#[derive(Debug)]
pub struct NseArchives {
    client: Client,
}

impl NseArchives {
    pub fn new() -> Result<Self> {
        let client = Client::builder().user_agent(USER_AGENT).build()?;
        Ok(Self { client })
    }

    /// Fetches the bhavcopy CSV text for a single trading day.
    pub async fn bhavcopy_raw(&self, dt: NaiveDate) -> Result<String> {
        let url = format!(
            "{BASE_URL}/content/cm/BhavCopy_NSE_CM_0_0_0_{}_F_0000.csv.zip",
            dt.format("%Y%m%d")
        );

        let response = self.client.get(&url).send().await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::NOT_FOUND => return Err(Error::NoData),
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => {
                return Err(Error::Parse(format!(
                    "unexpected response status {status} from {url}"
                )));
            }
        }

        let bytes = response.bytes().await?;
        unzip_single_csv(&bytes)
    }

    pub async fn bhavcopy_save(&self, dt: NaiveDate, dest: &Path) -> Result<PathBuf> {
        let file_name = format!("cm{}bhav.csv", dt.format("%d%b%Y"));
        let path = dest.join(file_name);

        if path.is_file() {
            return Ok(path);
        }

        let text = self.bhavcopy_raw(dt).await?;
        std::fs::write(&path, text)?;
        Ok(path)
    }
}

fn unzip_single_csv(data: &[u8]) -> Result<String> {
    let reader = Cursor::new(data);
    let mut archive =
        ZipArchive::new(reader).map_err(|e| Error::Parse(format!("bad zip archive: {e}")))?;

    let mut entry = archive
        .by_index(0)
        .map_err(|e| Error::Parse(format!("could not read zip entry: {e}")))?;

    let mut contents = String::new();
    entry
        .read_to_string(&mut contents)
        .map_err(|e| Error::Parse(format!("zip entry was not valid utf-8: {e}")))?;

    Ok(contents)
}
