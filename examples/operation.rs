use std::{
    borrow::Borrow,
    error::Error,
    fs::File,
    io::Write,
    path::Path,
    sync::Arc,
    thread::{sleep, spawn, JoinHandle},
    time::Duration,
};

use file_serve::Server;
use rand::{thread_rng, Rng};
use tempfile::tempdir;

use dlcommon::{
    http::{get_client, FileDownload},
    operation::Operation,
};

fn main() {
    let td = tempdir().expect("Should create temp directory");
    let temppath = td.path().to_owned();
    let files = create_temp_files(&temppath).unwrap();
    let (s, servehandle) = spawn_server(&temppath);
    download_items(s.addr(), &files).unwrap();
    s.close();
    println!("closed server!");
    servehandle.join().unwrap();
    drop(td);
}

fn download_items<S>(addr: &str, items: &[S]) -> Result<(), Box<dyn Error>>
where
    S: AsRef<str>,
{
    let dir = tempdir()?;
    let v: Vec<_> = items
        .into_iter()
        .map(|s| {
            let url = format!("http://{}/{}", addr, s.as_ref());
            FileDownload::builder()
                .url(url)
                .target(dir.path().to_owned())
                .title(s.as_ref().to_string())
                .filename(s.as_ref().to_string())
                .build()
                .unwrap()
        })
        .collect();
    let r = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();
    r.block_on(
        Operation::builder()
            .client(get_client(None)?)
            .wait_after_download(1)
            .concurrency(5)
            .build()
            .unwrap()
            .run(&v),
    )
    .unwrap();
    drop(dir);
    Ok(())
}

fn create_temp_files(temppath: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let mut rng = thread_rng();
    println!("setting up temp files in {}", temppath.display());
    let mut files = vec![];
    for i in 0..20 {
        let nm = format!("temp-file-{i}");
        let fname = temppath.join(&nm);
        let mut file = File::create(fname)?;
        let mut data = [0u8; 8];
        for _ in 1..rng.gen_range(64..=4096) {
            rng.fill(&mut data);
            file.write_all(&data)?;
        }
        files.push(nm);
    }
    Ok(files)
}

fn spawn_server(temppath: &Path) -> (Arc<Server>, JoinHandle<()>) {
    let s = Arc::new(file_serve::Server::new(&temppath));
    let t = s.clone();
    let servehandle = spawn(move || {
        println!("starting static file server at {}", t.addr());
        t.serve().unwrap();
    });
    // sleep to allow the other server to start
    sleep(Duration::from_secs(1));
    assert!(s.is_running());
    (s, servehandle)
}
