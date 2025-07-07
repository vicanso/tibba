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

use super::Error;
use dashmap::DashMap;
use headless_chrome::Browser;
use headless_chrome::protocol::cdp::Network::ResourceTiming;
use headless_chrome::protocol::cdp::Target::CreateTarget;
use headless_chrome::protocol::cdp::types::Event;
use headless_chrome::protocol::cdp::{Network, Page};
use headless_chrome::util::Wait;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Default)]
pub struct WebPageParams {
    pub url: String,
    pub cdp: String,
    pub width: u32,
    pub height: u32,
    pub wait_for_element: Option<String>,
    pub device_scale_factor: Option<f64>,
    pub timeout: Option<Duration>,
}

#[derive(Debug, Clone, Default)]
pub struct WebPageResource {
    pub content_size: f64,
    pub request_id: String,
    pub status: u32,
    pub url: String,
    pub timing: Option<ResourceTiming>,
    pub mime_type: String,
}

pub async fn run_webpage_stat(params: &WebPageParams) -> Result<()> {
    let browser = Browser::connect_with_timeout(
        params.cdp.clone(),
        params.timeout.unwrap_or(Duration::from_secs(120)),
    )
    .map_err(|e| Error::HeadlessChrome {
        message: e.to_string(),
    })?;

    let tab = browser
        .new_tab_with_options(CreateTarget {
            url: "about:blank".to_string(),
            width: Some(params.width),
            height: Some(params.height),
            browser_context_id: None,
            enable_begin_frame_control: None,
            new_window: Some(true),
            background: None,
            for_tab: None,
        })
        .map_err(|e| Error::HeadlessChrome {
            message: e.to_string(),
        })?;
    tab.call_method(Page::SetDeviceMetricsOverride {
        width: params.width,
        height: params.height,
        device_scale_factor: params.device_scale_factor.unwrap_or(1.0),
        mobile: true,
        screen_width: Some(params.width),
        screen_height: Some(params.height),
        position_x: None,
        position_y: None,
        dont_set_visible_size: None,
        scale: None,
        screen_orientation: None,
        viewport: None,
    })
    .map_err(|e| Error::HeadlessChrome {
        message: e.to_string(),
    })?;
    tab.enable_runtime().map_err(|e| Error::HeadlessChrome {
        message: e.to_string(),
    })?;
    tab.enable_fetch(None, None)
        .map_err(|e| Error::HeadlessChrome {
            message: e.to_string(),
        })?;
    tab.call_method(Network::Enable {
        max_total_buffer_size: None,
        max_resource_buffer_size: None,
        max_post_data_size: None,
    })
    .map_err(|e| Error::HeadlessChrome {
        message: e.to_string(),
    })?;
    let webpage_resources = Arc::new(DashMap::<String, WebPageResource>::new());
    let new_webpage_resources = webpage_resources.clone();
    let loaded = Arc::new(AtomicBool::new(false));
    let loaded_clone = loaded.clone();
    let listener = Arc::new(move |event: &Event| {
        // println!("event: {:?}", event);
        if let Event::PageLifecycleEvent(lifecycle) = event {
            if lifecycle.params.name == "load" {
                loaded_clone.store(true, Ordering::SeqCst);
            }
            return;
        }
        if let Event::NetworkResponseReceived(response) = event {
            let key = response.params.request_id.clone();
            let timing = response.params.response.timing.clone();
            new_webpage_resources.insert(
                key.clone(),
                WebPageResource {
                    request_id: key,
                    status: response.params.response.status,
                    url: response.params.response.url.clone(),
                    timing,
                    mime_type: response.params.response.mime_type.clone(),
                    ..Default::default()
                },
            );
            return;
        }
        if let Event::NetworkLoadingFinished(response) = event {
            let key = response.params.request_id.clone();
            if let Some(mut stat) = new_webpage_resources.get_mut(&key) {
                stat.content_size = response.params.encoded_data_length;
            }
            return;
        }
        if let Event::RuntimeExceptionThrown(exception) = event {
            println!("exception: {:?}", exception);
        }
    });
    tab.add_event_listener(listener)
        .map_err(|e| Error::HeadlessChrome {
            message: e.to_string(),
        })?;
    tab.navigate_to(&params.url)
        .map_err(|e| Error::HeadlessChrome {
            message: e.to_string(),
        })?;
    if let Some(wait_for_element) = &params.wait_for_element {
        tab.wait_for_element(wait_for_element)
            .map_err(|e| Error::HeadlessChrome {
                message: e.to_string(),
            })?;
    } else {
        Wait::with_timeout(Duration::from_secs(60))
            .until(|| {
                if loaded.load(Ordering::SeqCst) {
                    Some(true)
                } else {
                    None
                }
            })
            .map_err(|e| Error::HeadlessChrome {
                message: e.to_string(),
            })?;
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    let _ = tab.close_with_unload();

    // println!("webpage_stats: {webpage_stats:?}");

    // let jpeg_data = tab.capture_screenshot(
    //     Page::CaptureScreenshotFormatOption::Jpeg,
    //     None,
    //     Some(Page::Viewport {
    //         x: 0.0,
    //         y: 0.0,
    //         width: 390.0,
    //         height: 844.0,
    //         scale: 1.0,
    //     }),
    //     true,
    // )?;
    Ok(())
}
