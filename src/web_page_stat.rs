// Copyright 2025 Tree xie.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use super::dal::get_opendal_storage;
use super::sql::get_db_pool;
use async_trait::async_trait;
use ctor::ctor;
use serde::Deserialize;
use tibba_error::Error;
use tibba_headless::{WebPageParams, new_browser, run_web_page_stat_with_browser};
use tibba_hook::{Task, register_task};
use tibba_model::{ConfigurationModel, FileInsertParams, FileModel, Model, WebPageDetectorModel};
use tibba_scheduler::{Job, register_job_task};
use tibba_util::uuid;
use tracing::{error, info};

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Deserialize, Default)]
struct BrowserLessConfig {
    urls: Vec<String>,
}

async fn run_web_page_stat() -> Result<()> {
    let pool = get_db_pool();
    let detectors = WebPageDetectorModel::new()
        .list_enabled_by_region(pool, None, 100, 0)
        .await?;

    if detectors.is_empty() {
        return Ok(());
    }

    let browser_less_config: BrowserLessConfig = ConfigurationModel::new()
        .get_config(pool, "app", "browserless")
        .await
        .map_err(|e| Error::new(e.to_string()))?;
    if browser_less_config.urls.is_empty() {
        return Err(Error::new("browser less urls is empty"));
    }
    let browser = new_browser(&browser_less_config.urls[0], None)?;

    for detector in detectors {
        println!("detector: {detector:?}");
        let mut params = WebPageParams {
            url: detector.url,
            width: detector.width,
            height: detector.height,
            capture_screenshot: detector.capture_screenshot,
            ..Default::default()
        };
        if !detector.user_agent.is_empty() {
            params.user_agent = Some(detector.user_agent);
        }
        let stat = run_web_page_stat_with_browser(&browser, &params)?;

        if let Some(screenshot) = stat.screenshot {
            let file = format!("{}.png", uuid());
            let storage = get_opendal_storage();
            let file_size = screenshot.data.len() as i64;
            let _ = storage.write_with(&file, screenshot.data, vec![]).await?;
            let params = FileInsertParams {
                group: "web_page_stat".to_string(),
                filename: file.clone(),
                file_size,
                content_type: "image/png".to_string(),
                uploader: "system".to_string(),
                width: Some(screenshot.width as i32),
                height: Some(screenshot.height as i32),
                ..Default::default()
            };
            let _ = FileModel::new().insert_file(pool, params).await?;
            println!("file: {file}");
        }
    }

    Ok(())
}

struct WebPageStatTask;

#[async_trait]
impl Task for WebPageStatTask {
    async fn before(&self) -> Result<bool> {
        // 每分钟
        let job = Job::new_async("30 * * * * *", |_, _| {
            let category = "web_page_stat";
            Box::pin(async move {
                match run_web_page_stat().await {
                    Err(e) => {
                        error!(
                            category,
                            error = ?e,
                            "run web page stat failed"
                        );
                    }
                    Ok(()) => {
                        info!(category, "run web page stat success");
                    }
                };
            })
        })
        .map_err(Error::new)?;
        register_job_task("web_page_stat", job);
        Ok(true)
    }
    fn priority(&self) -> u8 {
        u8::MAX
    }
}
#[ctor]
fn init() {
    register_task("web_page_stat", Box::new(WebPageStatTask));
}
