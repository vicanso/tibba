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
use axum::routing::get;
use schemars::Schema;
use serde::Deserialize;
use tibba_model::{File, User};
use tibba_util::{JsonResult, Query};
use tibba_validator::x_schema_name;
use validator::Validate;

#[derive(Debug, Deserialize, Clone, Validate)]
struct GetSchemaParams {
    #[validate(custom(function = "x_schema_name"))]
    name: String,
}

async fn get_schema(Query(params): Query<GetSchemaParams>) -> JsonResult<Schema> {
    let schema = match params.name.as_str() {
        "user" => User::schema(),
        _ => File::schema(),
    };
    Ok(Json(schema))
}

pub fn new_schema_router() -> Router {
    Router::new().route("/json", get(get_schema))
}
