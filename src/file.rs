use std::{
    error::Error,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

use rand::{distributions::Alphanumeric, thread_rng, Rng};
use tokio::{
    fs::{remove_file, rename, File},
    io::AsyncWriteExt,
    spawn,
};

pub fn temp_path(p: &Path) -> Option<PathBuf> {
    let o = temp_filename(p.file_name()?);
    Some(p.parent().map_or_else(|| PathBuf::from(&o), |i| i.join(&o)))
}

pub fn temp_filename(filename: &OsStr) -> OsString {
    let period = OsStr::new(".");
    let mut rng = thread_rng();
    let suffix: Vec<_> = (0..8).map(|_| rng.sample(Alphanumeric)).collect();
    let s = unsafe { OsString::from_encoded_bytes_unchecked(suffix) };
    vec![period, filename, period, OsStr::new("tmp"), &s]
        .into_iter()
        .collect()
}

pub struct AtomicFile {
    file: File,
    temp_path: PathBuf,
    target_path: PathBuf,
    committed: bool,
}

impl AtomicFile {
    pub async fn open<P>(p: P) -> Result<Self, Box<dyn Error>>
    where
        P: AsRef<Path>,
    {
        let target_path = p.as_ref().to_owned();
        let temp_path = temp_path(&target_path).ok_or("Should be a regular file")?;
        let file = File::options()
            .create_new(true)
            .write(true)
            .open(&temp_path)
            .await?;
        Ok(AtomicFile {
            file,
            temp_path,
            target_path,
            committed: false,
        })
    }
    pub async fn write_all(&mut self, data: &[u8]) -> Result<(), Box<dyn Error>> {
        Ok(self.file.write_all(data).await?)
    }
    pub async fn commit(&mut self) -> Result<(), Box<dyn Error>> {
        if self.committed {
            return Ok(());
        }
        self.committed = true;
        self.file.sync_all().await?;
        rename(&self.temp_path, &self.target_path).await?;
        Ok(())
    }
    pub async fn discard(&mut self) -> Result<(), Box<dyn Error>> {
        if self.committed {
            return Ok(());
        }
        Ok(remove_file(&self.target_path).await?)
    }
}

impl Drop for AtomicFile {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let p = self.temp_path.clone();
        // TODO: should we just do this sync as this only happens in an error condition?!
        spawn(async move {
            let _ = remove_file(&p).await;
        });
    }
}
