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

use axum::Json;
use axum::Router;
use axum::extract::Multipart;
use axum::extract::State;
use axum::http::HeaderValue;
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use imageoptimize::ProcessImage;
use serde::Deserialize;
use sqlx::MySqlPool;
use std::collections::HashMap;
use std::path::Path;
use tibba_error::{Error, new_error};
use tibba_model::{Configuration, File, FileInsertParams};
use tibba_opendal::Storage;
use tibba_session::UserSession;
use tibba_util::{JsonResult, QueryParams, uuid};
use tibba_validator::{x_file_group, x_file_name, x_image_format, x_image_quality};
use validator::Validate;

type Result<T> = std::result::Result<T, Error>;

const ERROR_CATEGORY: &str = "file_router";

#[derive(Debug, Deserialize, Clone, Validate)]
struct CreateFileParams {
    #[validate(custom(function = "x_file_group"))]
    group: String,
}

async fn create_file(
    QueryParams(create_file_params): QueryParams<CreateFileParams>,
    State((storage, pool)): State<(&'static Storage, &'static MySqlPool)>,
    session: UserSession,
    mut multipart: Multipart,
) -> JsonResult<HashMap<String, String>> {
    let mut files = HashMap::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| new_error(e).with_category(ERROR_CATEGORY))?
    {
        let name = field.name().unwrap_or_default().to_string();
        let file_name = field.file_name().unwrap_or_default().to_string();
        if name.is_empty() && file_name.is_empty() {
            continue;
        }
        let ext = Path::new(&file_name)
            .extension()
            .unwrap_or_default()
            .to_string_lossy();
        if ext.is_empty() {
            continue;
        }
        let file = format!("{}.{}", uuid(), ext);

        let data = field
            .bytes()
            .await
            .map_err(|e| new_error(e).with_category(ERROR_CATEGORY))?;
        let content_type = mime_guess::from_path(&file_name).first_or_octet_stream();
        let mut params = FileInsertParams {
            group: create_file_params.group.clone(),
            filename: file.clone(),
            file_size: data.len() as i64,
            content_type: content_type.to_string(),
            uploader: session.get_account(),
            ..Default::default()
        };

        if content_type.type_() == "image" {
            let format = image::guess_format(&data)
                .map_err(|e| new_error(e).with_category(ERROR_CATEGORY))?;
            params.content_type = format.to_mime_type().to_string();
            let image = image::load_from_memory_with_format(&data, format)
                .map_err(|e| new_error(e).with_category(ERROR_CATEGORY))?;
            params.width = Some(image.width() as i32);
            params.height = Some(image.height() as i32);
        };

        let _ = storage.write_with(&file, data.clone(), vec![]).await?;
        let _ = File::insert(pool, params).await?;

        files.insert(name.to_string(), file);
    }
    Ok(Json(files))
}

#[derive(Debug, Deserialize, Clone, Validate)]
struct GetFileParams {
    #[validate(custom(function = "x_file_name"))]
    name: String,
    #[validate(custom(function = "x_image_format"))]
    format: Option<String>,
    #[validate(custom(function = "x_image_quality"))]
    quality: Option<u8>,
    optimize: Option<bool>,
    width: Option<u32>,
    height: Option<u32>,
}

async fn get_file(
    State((storage, pool)): State<(&'static Storage, &'static MySqlPool)>,
    QueryParams(params): QueryParams<GetFileParams>,
) -> Result<impl IntoResponse> {
    let file = File::get_by_name(pool, &params.name)
        .await?
        .ok_or(new_error("file not found").with_category(ERROR_CATEGORY))?;
    let mut data = storage.read(&params.name).await?;
    let mut content_type = file.content_type.clone();
    let ext = content_type.split("/").last().unwrap_or_default();
    let format = params.format.unwrap_or(ext.to_string());
    let mut headers = header::HeaderMap::with_capacity(8);
    if params.optimize.unwrap_or(true) && content_type.starts_with("image") {
        let image = ProcessImage::new(data.to_vec(), ext)
            .map_err(|e| new_error(e).with_category(ERROR_CATEGORY))?;
        let mut tasks = vec![];
        if params.width.is_some() || params.height.is_some() {
            tasks.push(vec![
                "resize".to_string(),
                params.width.unwrap_or(0).to_string(),
                params.height.unwrap_or(0).to_string(),
            ]);
        }
        tasks.push(vec![
            "optim".to_string(),
            format.clone(),
            params.quality.unwrap_or(80).to_string(),
            "3".to_string(),
        ]);
        tasks.push(vec!["diff".to_string()]);

        let image = imageoptimize::run_with_image(image, tasks)
            .await
            .map_err(|e| new_error(e).with_category(ERROR_CATEGORY))?;

        if let Ok(diff) = HeaderValue::from_str(&format!("{:.2}", image.diff)) {
            headers.insert("X-Diff", diff);
        }

        data = image
            .get_buffer()
            .map_err(|e| new_error(e).with_category(ERROR_CATEGORY))?
            .into();
        if let Some(mime_type) = mime_guess::from_ext(format.as_str()).first() {
            content_type = mime_type.to_string();
        }
    }

    if let Ok(header_value) = HeaderValue::from_str(&content_type) {
        headers.insert(header::CONTENT_TYPE, header_value);
    }
    let size = data.len();
    if size > 0 {
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from(size));
    }
    if let Some(metadata) = file.get_metadata() {
        headers.extend(metadata);
    }
    let Some(response_headers) = Configuration::get_response_headers(pool, &file.group).await?
    else {
        return Ok((headers, data.to_bytes()));
    };
    headers.extend(response_headers);

    Ok((headers, data.to_bytes()))
}

pub struct FileRouterParams {
    pub storage: &'static Storage,
    pub pool: &'static MySqlPool,
}

pub fn new_file_router(params: FileRouterParams) -> Router {
    Router::new()
        .route(
            "/upload",
            post(create_file).with_state((params.storage, params.pool)),
        )
        .route(
            "/preview",
            get(get_file).with_state((params.storage, params.pool)),
        )
}
