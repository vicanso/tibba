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
use ctor::ctor;
use serde::Deserialize;
use tibba_error::{Error, new_error};
use tibba_headless::{WebPageParams, new_browser, run_web_page_stat_with_browser};
use tibba_hook::register_before_task;
use tibba_model::{Configuration, File, FileInsertParams, WebPageDetector};
use tibba_scheduler::{Job, register_job_task};
use tibba_util::uuid;
use tracing::{error, info};

#[derive(Debug, Clone, Deserialize, Default)]
struct BrowserLessConfig {
    urls: Vec<String>,
}

async fn run_web_page_stat() -> Result<(), Error> {
    let pool = get_db_pool();
    let browser_less_config: BrowserLessConfig =
        Configuration::get_config(pool, "app", "browserless")
            .await
            .map_err(|e| new_error(e.to_string()))?;
    if browser_less_config.urls.is_empty() {
        return Err(new_error("browser less urls is empty").into());
    }
    let browser =
        new_browser(&browser_less_config.urls[0], None).map_err(|e| new_error(e.to_string()))?;

    let detectors = WebPageDetector::list_enabled_by_region(pool, None, 100, 0).await?;
    for detector in detectors {
        println!("{:?}", detector);
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
        let stat = run_web_page_stat_with_browser(&browser, &params)
            .map_err(|e| new_error(e.to_string()))?;

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
            let _ = File::insert(pool, params).await?;
            println!("file: {file}");
        }
    }

    Ok(())
}

#[ctor]
fn init() {
    register_before_task(
        "init_web_page_stat",
        u8::MAX,
        Box::new(|| {
            Box::pin(async {
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
                .map_err(new_error)?;
                register_job_task("web_page_stat", job);
                Ok(())
            })
        }),
    );
}
