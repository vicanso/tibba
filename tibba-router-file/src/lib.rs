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
use axum::http::header;
use axum::http::{HeaderName, HeaderValue};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use serde::Deserialize;
use sqlx::MySqlPool;
use std::collections::HashMap;
use std::path::Path;
use tibba_error::{Error, new_error};
use tibba_model::{File, FileInsertParams};
use tibba_opendal::Storage;
use tibba_session::UserSession;
use tibba_util::{JsonResult, Query, uuid};
use tibba_validator::{x_file_group, x_file_name};
use validator::Validate;

type Result<T> = std::result::Result<T, Error>;

const ERROR_CATEGORY: &str = "file_router";

#[derive(Debug, Deserialize, Clone, Validate)]
pub struct CreateFileParams {
    #[validate(custom(function = "x_file_group"))]
    pub group: String,
}

async fn create_file(
    Query(create_file_params): Query<CreateFileParams>,
    State((storage, pool)): State<(&'static Storage, &'static MySqlPool)>,
    session: UserSession,
    mut multipart: Multipart,
) -> JsonResult<HashMap<String, String>> {
    let mut files = HashMap::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| new_error(&e.to_string()).with_category(ERROR_CATEGORY))?
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
            .map_err(|e| new_error(&e.to_string()).with_category(ERROR_CATEGORY))?;
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
            let image = image::load_from_memory(&data)
                .map_err(|e| new_error(&e.to_string()).with_category(ERROR_CATEGORY))?;
            params.width = Some(image.width());
            params.height = Some(image.height());
        };

        let _ = storage.write_with(&file, data.clone(), vec![]).await?;
        let _ = File::insert(pool, params).await?;

        // let user_metadata = vec![
        //     (
        //         header::CACHE_CONTROL.to_string(),
        //         "public, max-age=108000".to_string(),
        //     ),
        //     ("uploader".to_string(), session.get_account()),
        // ];
        files.insert(name.to_string(), file);
    }
    Ok(Json(files))
}

#[derive(Debug, Deserialize, Clone, Validate)]
pub struct GetFileParams {
    #[validate(custom(function = "x_file_name"))]
    pub name: String,
}

async fn get_file(
    State(storage): State<&'static Storage>,
    Query(params): Query<GetFileParams>,
) -> Result<impl IntoResponse> {
    let stat = storage.stat(&params.name).await?;
    let data = storage.read(&params.name).await?;

    let mut headers = header::HeaderMap::with_capacity(4);
    if let Some(content_type) = stat.content_type() {
        if let Ok(header_value) = HeaderValue::from_str(content_type) {
            headers.insert(header::CONTENT_TYPE, header_value);
        }
    }
    let size = stat.content_length();
    if size > 0 {
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from(size));
    }
    let ignore_headers = ["uploader".to_string()];

    if let Some(user_metadata) = stat.user_metadata() {
        for (key, value) in user_metadata {
            if ignore_headers.contains(key) {
                continue;
            }
            let Ok(key) = HeaderName::from_bytes(key.as_bytes()) else {
                continue;
            };
            if let Ok(value) = HeaderValue::from_str(value) {
                headers.insert(key, value);
            }
        }
    }

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
        .route("/preview", get(get_file).with_state(params.storage))
}
