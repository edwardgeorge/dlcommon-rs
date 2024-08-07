use std::{error::Error, sync::Arc, time::Duration};

use derive_builder::Builder;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use reqwest::Client;
use tokio::{
    spawn,
    sync::{OwnedSemaphorePermit, Semaphore},
    time::sleep,
};

use crate::http::FileDownload;
use crate::style::*;

#[derive(Clone, Builder)]
pub struct Operation {
    #[builder(setter(into))]
    client: Arc<Client>,
    #[builder(default = "Arc::new(Semaphore::new(1))", setter(custom))]
    concurrency: Arc<Semaphore>,
    #[builder(default, setter(into, strip_option))]
    multiprogress: Option<Arc<MultiProgress>>,
    #[builder(default = "Duration::from_secs(1)", setter(custom))]
    wait_after_download: Duration,
    #[builder(default, setter(into, strip_option))]
    main_progress_style: Option<ProgressStyle>,
    #[builder(default, setter(into, strip_option))]
    spin_progress_style: Option<ProgressStyle>,
    #[builder(default, setter(into, strip_option))]
    item_progress_style: Option<ProgressStyle>,
    #[builder(default, setter(into, strip_option))]
    item_success_style: Option<ProgressStyle>,
    #[builder(default, setter(into, strip_option))]
    item_failure_style: Option<ProgressStyle>,
}

impl OperationBuilder {
    pub fn wait_after_download(&mut self, secs: u64) -> &mut Self {
        self.wait_after_download = Some(Duration::from_secs(secs));
        self
    }
    pub fn concurrency(&mut self, n: usize) -> &mut Self {
        self.concurrency = Some(Arc::new(Semaphore::new(n)));
        self
    }
    pub fn with_semaphore(&mut self, sem: Arc<Semaphore>) -> &mut Self {
        self.concurrency = Some(sem);
        self
    }
}

impl Operation {
    pub fn builder() -> OperationBuilder {
        OperationBuilder::default()
    }
    pub async fn run(&self, items: &[FileDownload]) -> Result<(), Box<dyn Error>> {
        let mut handles = vec![];
        let mult = self
            .multiprogress
            .as_ref()
            .cloned()
            .unwrap_or_else(|| Arc::new(MultiProgress::new()));
        let totalprogress = Arc::new(
            mult.add(
                ProgressBar::new(items.len() as u64).with_style(
                    self.main_progress_style
                        .as_ref()
                        .unwrap_or_else(|| main_progress_style())
                        .clone(),
                ),
            ),
        );
        let spin_style = self
            .spin_progress_style
            .as_ref()
            .unwrap_or_else(|| spin_progress_style());
        let item_style = self
            .item_progress_style
            .as_ref()
            .unwrap_or_else(|| item_progress_style());
        let success_style = self
            .item_success_style
            .as_ref()
            .unwrap_or_else(|| item_success_style());
        let failure_style = self
            .item_failure_style
            .as_ref()
            .unwrap_or_else(|| item_failure_style());

        for file_dl in items {
            // obtain semaphore
            let ticket = self.concurrency.clone().acquire_owned().await?;
            let jh = spawn(create_task(
                ticket,
                self.client.clone(),
                file_dl.clone(),
                mult.clone(),
                totalprogress.clone(),
                spin_style.clone(),
                item_style.clone(),
                success_style.clone(),
                failure_style.clone(),
                self.wait_after_download,
            ));
            handles.push(jh);
        }
        for h in handles {
            h.await?;
        }
        totalprogress.finish();
        Ok(())
    }
}

async fn create_task(
    ticket: OwnedSemaphorePermit,
    client: Arc<Client>,
    file_dl: FileDownload,
    mult: Arc<MultiProgress>,
    totalprogress: Arc<ProgressBar>,
    spin_style: ProgressStyle,
    item_style: ProgressStyle,
    success_style: ProgressStyle,
    failure_style: ProgressStyle,
    wait_duration: Duration,
) {
    let spinner = mult.add(
        ProgressBar::new_spinner()
            .with_style(spin_style)
            .with_message(
                file_dl
                    .title
                    .as_ref()
                    .cloned()
                    .unwrap_or_else(|| "Setting up download".to_string()),
            ),
    );
    spinner.enable_steady_tick(Duration::from_millis(100));
    let mut progress: Option<ProgressBar> = None;
    let title = file_dl
        .title
        .as_ref()
        .cloned()
        .unwrap_or_else(|| file_dl.url.clone());
    match file_dl
        .download(
            &client,
            Some(|len, pos| {
                if let Some(p) = &progress {
                    p.set_position(pos);
                } else {
                    spinner.finish();
                    let p =
                        mult.insert_after(
                            &spinner,
                            ProgressBar::new(len)
                                .with_message(file_dl.title.as_ref().cloned().unwrap_or_else(
                                    || format!("Downloading from '{}'", &file_dl.url),
                                ))
                                .with_style(item_style.clone()),
                        );
                    mult.remove(&spinner);
                    p.set_position(pos);
                    progress.replace(p);
                }
            }),
        )
        .await
    {
        Ok(_) => {
            if let Some(p) = progress {
                p.set_style(success_style);
                p.finish();
            }
        }
        Err(e) => {
            if let Some(p) = progress {
                p.set_style(failure_style);
                p.abandon();
            }
            mult.suspend(|| {
                eprintln!("Error downloading '{}': {e}", title);
            });
        }
    }
    totalprogress.inc(1);
    sleep(wait_duration).await;
    // we wanted to move here, so it is within this scope.
    // explicitly dropping does this for us
    drop(ticket);
}
