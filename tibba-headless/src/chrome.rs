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
use headless_chrome::Tab;
use headless_chrome::protocol::cdp::Network::ResourceTiming;
use headless_chrome::protocol::cdp::Target::CreateTarget;
use headless_chrome::protocol::cdp::types::Event;
use headless_chrome::protocol::cdp::{Network, Page};
use headless_chrome::util::Wait;
use palette::{IntoColor, Luv, Srgb};
use scopeguard::defer;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

type Result<T> = std::result::Result<T, Error>;

/// 将LUV颜色转换为256个级别，按照人眼视觉区分度划分
///
/// 基于CIELUV颜色空间的感知均匀特性，将L、u、v分量映射到256个级别：
/// - L分量（亮度）：分配128个级别，因为人眼对亮度变化最敏感
/// - u分量（色度）：分配64个级别
/// - v分量（色度）：分配64个级别
///
/// 这种分配方式考虑了人眼的视觉特性：
/// - 人眼对亮度变化比色度变化更敏感
/// - 在低亮度区域，人眼对色度变化更敏感
/// - 在高亮度区域，人眼对亮度变化更敏感
fn luv_to_byte(luv: &Luv) -> u8 {
    // 获取L、u、v分量
    let l = luv.l;
    let u = luv.u;
    let v = luv.v;

    // 处理无效值
    if l.is_nan() || u.is_nan() || v.is_nan() {
        return 0;
    }

    // 限制L值范围到0-100
    let l_clamped = l.clamp(0.0, 100.0);

    // 限制u、v值到合理范围（通常-100到100）
    let u_clamped = u.clamp(-100.0, 100.0);
    let v_clamped = v.clamp(-100.0, 100.0);

    // 使用感知均匀的映射方式
    // 亮度分量：使用非线性映射，在低亮度区域分配更多级别
    let l_normalized = if l_clamped < 50.0 {
        // 低亮度区域：使用平方根映射，分配更多级别
        (l_clamped / 50.0).powf(0.5) * 0.6
    } else {
        // 高亮度区域：使用线性映射
        0.6 + (l_clamped - 50.0) / 50.0 * 0.4
    };

    // 色度分量：使用感知均匀的映射
    let u_normalized = (u_clamped + 100.0) / 200.0;
    let v_normalized = (v_clamped + 100.0) / 200.0;

    // 组合三个分量到256个级别
    // 使用加权组合，亮度权重更高
    let l_weight = 0.6; // 亮度权重60%
    let u_weight = 0.2; // u色度权重20%
    let v_weight = 0.2; // v色度权重20%

    let combined_value =
        l_normalized * l_weight + u_normalized * u_weight + v_normalized * v_weight;

    // 转换为0-255范围
    (combined_value * 255.0) as u8
}

#[derive(Debug, Clone, Default)]
pub struct WebPageParams {
    pub url: String,
    pub width: u32,
    pub height: u32,
    pub user_agent: Option<String>,
    pub accept_language: Option<String>,
    pub platform: Option<String>,
    pub wait_for_element: Option<String>,
    pub device_scale_factor: Option<f64>,
    pub timeout: Option<Duration>,
    pub capture_screenshot: bool,
    pub capture_element: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct WebPageStat {
    pub total_size: u64,
    pub fcp_time: u32,
    pub dcl_time: u32,
    pub load_time: u32,
    pub exceptions: Vec<String>,
    pub resources: Vec<WebPageResource>,
    pub image_data: Option<Vec<u8>>,
    pub color_percents: Option<Vec<Vec<u8>>>,
}

#[derive(Debug, Clone, Default)]
pub struct WebPageResource {
    pub content_size: u64,
    pub request_id: String,
    pub status: u32,
    pub url: String,
    pub timing: Option<ResourceTiming>,
    pub mime_type: String,
    pub connection_reused: bool,
}

#[derive(Debug, Clone, Default)]
pub struct WebPageLifecycle {
    pub init_time: f64,
    pub fcp_time: f64,
    pub dcl_time: f64,
    pub load_time: f64,
}

fn analyze_web_page_screenshot(
    tab: Arc<Tab>,
    params: &WebPageParams,
) -> Result<(Vec<u8>, Vec<Vec<u8>>)> {
    let image_data = if let Some(capture_element) = &params.capture_element {
        tab.wait_for_element(capture_element)
            .map_err(|e| Error::HeadlessChrome {
                message: e.to_string(),
            })?
            .capture_screenshot(Page::CaptureScreenshotFormatOption::Png)
            .map_err(|e| Error::HeadlessChrome {
                message: e.to_string(),
            })?
    } else {
        tab.capture_screenshot(
            Page::CaptureScreenshotFormatOption::Png,
            Some(90),
            Some(Page::Viewport {
                x: 0.0,
                y: 0.0,
                width: params.width as f64,
                height: params.height as f64,
                scale: 1.0,
            }),
            true,
        )
        .map_err(|e| Error::HeadlessChrome {
            message: e.to_string(),
        })?
    };

    let img =
        image::load_from_memory_with_format(&image_data, image::ImageFormat::Png).map_err(|e| {
            Error::HeadlessChrome {
                message: e.to_string(),
            }
        })?;
    let mut color_percents = vec![];
    if let Some(img) = img.as_rgba8() {
        let luv_list = img
            .pixels()
            .map(|pixel| {
                let rgb = Srgb::new(pixel[0], pixel[1], pixel[2]);
                let luv: Luv = rgb.into_linear().into_color();
                luv
            })
            .collect::<Vec<_>>();
        let mut color_count: [u64; 256] = [0; 256];
        for luv in luv_list.iter() {
            let value = luv_to_byte(luv);
            color_count[value as usize] += 1;
        }
        let count = luv_list.len() as f64;
        for (index, item) in color_count.iter().enumerate() {
            let value = (*item as f64) * 100.0 / count;
            if value < 0.5 {
                continue;
            }
            let value = value.ceil() as u8;
            color_percents.push((index, value));
        }
    }
    Ok((
        image_data,
        color_percents
            .iter()
            .map(|item| vec![item.0 as u8, item.1])
            .collect(),
    ))
}

pub async fn run_web_page_stat_with_browser(
    browser: Browser,
    params: &WebPageParams,
) -> Result<WebPageStat> {
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
    defer!(let _ = tab.close_with_unload(););
    if let Some(user_agent) = &params.user_agent {
        tab.set_user_agent(
            user_agent,
            params.accept_language.as_deref(),
            params.platform.as_deref(),
        )
        .map_err(|e| Error::HeadlessChrome {
            message: e.to_string(),
        })?;
    }
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
    let web_page_resources = Arc::new(DashMap::<String, WebPageResource>::new());
    let web_page_resources_clone = web_page_resources.clone();
    let exceptions = Arc::new(Mutex::new(Vec::new()));
    let exceptions_clone = exceptions.clone();
    let loaded = Arc::new(AtomicBool::new(false));
    let loaded_clone = loaded.clone();
    let lifecycle = Arc::new(Mutex::new(WebPageLifecycle::default()));
    let lifecycle_clone = lifecycle.clone();

    let listener = Arc::new(move |event: &Event| {
        if let Event::PageLifecycleEvent(lifecycle) = event {
            let params = &lifecycle.params;
            match params.name.as_str() {
                "init" => {
                    if let Ok(mut lifecycle) = lifecycle_clone.lock() {
                        if lifecycle.init_time == 0.0 {
                            lifecycle.init_time = params.timestamp;
                        }
                    }
                }
                "load" => {
                    if let Ok(mut lifecycle) = lifecycle_clone.lock() {
                        lifecycle.load_time = params.timestamp;
                    }
                    loaded_clone.store(true, Ordering::SeqCst);
                }
                "firstContentfulPaint" => {
                    if let Ok(mut lifecycle) = lifecycle_clone.lock() {
                        if lifecycle.fcp_time == 0.0 {
                            lifecycle.fcp_time = params.timestamp;
                        }
                    }
                }
                "DOMContentLoaded" => {
                    if let Ok(mut lifecycle) = lifecycle_clone.lock() {
                        if lifecycle.dcl_time == 0.0 {
                            lifecycle.dcl_time = params.timestamp;
                        }
                    }
                }
                _ => {}
            }
            return;
        }
        if let Event::NetworkResponseReceived(response) = event {
            let key = response.params.request_id.clone();
            let timing = response.params.response.timing.clone();
            web_page_resources_clone.insert(
                key.clone(),
                WebPageResource {
                    request_id: key,
                    status: response.params.response.status,
                    url: response.params.response.url.clone(),
                    timing,
                    mime_type: response.params.response.mime_type.clone(),
                    connection_reused: response.params.response.connection_reused,
                    ..Default::default()
                },
            );
            return;
        }
        if let Event::NetworkLoadingFinished(response) = event {
            let key = response.params.request_id.clone();
            if let Some(mut stat) = web_page_resources_clone.get_mut(&key) {
                stat.content_size = response.params.encoded_data_length as u64;
            }
            return;
        }
        if let Event::RuntimeExceptionThrown(exception) = event {
            let details = &exception.params.exception_details;
            let mut description = "".to_string();
            if let Some(exception) = &details.exception {
                description = exception.description.clone().unwrap_or_default();
            }
            let message = format!(
                "text: {}, line:{}, column:{}, description:{}",
                details.text, details.line_number, details.column_number, description
            );
            if let Ok(mut exceptions) = exceptions_clone.lock() {
                exceptions.push(message);
            }
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
        tokio::time::sleep(Duration::from_secs(3)).await;
    }

    let mut stat = WebPageStat {
        ..Default::default()
    };

    if let Ok(exceptions) = exceptions.lock() {
        stat.exceptions = exceptions.clone();
    }
    stat.resources = web_page_resources
        .iter()
        .map(|item| item.value().clone())
        .collect();
    for item in stat.resources.iter() {
        stat.total_size += item.content_size;
    }
    if let Ok(lifecycle) = lifecycle.lock() {
        if lifecycle.init_time > 0.0 && lifecycle.fcp_time > 0.0 {
            stat.fcp_time = (1000.0 * (lifecycle.fcp_time - lifecycle.init_time)) as u32;
        }
        if lifecycle.init_time > 0.0 && lifecycle.dcl_time > 0.0 {
            stat.dcl_time = (1000.0 * (lifecycle.dcl_time - lifecycle.init_time)) as u32;
        }
        if lifecycle.init_time > 0.0 && lifecycle.load_time > 0.0 {
            stat.load_time = (1000.0 * (lifecycle.load_time - lifecycle.init_time)) as u32;
        }
    }

    if params.capture_screenshot {
        if let Ok((image_data, color_percents)) = analyze_web_page_screenshot(tab.clone(), params) {
            stat.image_data = Some(image_data);
            stat.color_percents = Some(color_percents);
        }
    }

    Ok(stat)
}

pub fn new_browser(cdp: &str, timeout: Option<Duration>) -> Result<Browser> {
    let browser =
        Browser::connect_with_timeout(cdp.to_string(), timeout.unwrap_or(Duration::from_secs(120)))
            .map_err(|e| Error::HeadlessChrome {
                message: e.to_string(),
            })?;
    Ok(browser)
}
