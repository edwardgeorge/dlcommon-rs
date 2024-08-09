use std::{error::Error, fs::File, io::Write, path::Path, time::Duration};

use actix_web::dev::ServerHandle;
use rand::{thread_rng, Rng};
use tempfile::tempdir;

use dlcommon::{
    http::{get_client, FileDownload},
    operation::Operation,
};
use tokio::{spawn, task::JoinHandle, time::sleep};

#[tokio::main]
async fn main() {
    let td = tempdir().expect("Should create temp directory");
    let temppath = td.path().to_owned();
    let files = create_temp_files(&temppath).unwrap();
    let (servehandle, joinhandle) = create_server(&temppath).unwrap();
    download_items("127.0.0.1:8080", &files).await.unwrap();
    println!("finished. shutting down web server!");
    servehandle.stop(false).await;
    joinhandle.await.unwrap().unwrap();
    drop(td);
}

async fn download_items<S>(addr: &str, items: &[S]) -> Result<(), Box<dyn Error>>
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
                .preflight_head(true)
                .target(dir.path().to_owned())
                .title(s.as_ref().to_string())
                .filename(s.as_ref().to_string())
                .build()
                .unwrap()
        })
        .collect();
    Operation::builder()
        .client(get_client(None)?)
        .wait_after_download(1)
        .concurrency(5)
        .build()
        .unwrap()
        .run(&v[..])
        .await?;
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
        let mut data = [0u8; 1024];
        for _ in 1..rng.gen_range(64..=4096) {
            rng.fill(&mut data);
            file.write_all(&data)?;
        }
        file.sync_all()?;
        drop(file);
        files.push(nm);
    }
    for f in &files {
        let s = temppath.join(f).metadata().unwrap().len();
        println!("wrote '{}', size: {}", f, s);
    }
    Ok(files)
}

fn create_server(
    temppath: &Path,
) -> std::io::Result<(ServerHandle, JoinHandle<Result<(), std::io::Error>>)> {
    use actix_files::Files;
    use actix_web::{App, HttpServer};

    let p = temppath.to_owned();
    let s = HttpServer::new(move || {
        App::new().service(Files::new("/", p.clone()).show_files_listing())
    })
    .bind(("127.0.0.1", 8080))?
    .run();
    let sh = s.handle();
    let jh = spawn(s);
    Ok((sh, jh))
}
