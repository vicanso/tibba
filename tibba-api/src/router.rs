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

use crate::cache::get_redis_cache;
use crate::config::must_get_basic_config;
use crate::dal::get_opendal_storage;
use crate::sql::get_db_pool;
use crate::state::get_app_state;
use axum::Router;
use tibba_error::Error;
use tibba_router_common::{CommonRouterParams, new_common_router};
use tibba_router_file::{FileRouterParams, new_file_router};
use tibba_router_schema::new_schema_router;
use tibba_router_user::{UserRouterParams, new_user_router};
use tibba_util::{is_development, is_test};

type Result<T> = std::result::Result<T, Error>;

pub fn new_router() -> Result<Router> {
    let basic_config = must_get_basic_config();
    let cache = get_redis_cache();
    let common_router = new_common_router(CommonRouterParams {
        state: get_app_state(),
        secret: basic_config.secret.clone(),
        cache,
    });
    let mut magic_code = "".to_string();
    // only for test
    if is_test() || is_development() {
        magic_code = "1234".to_string();
    }
    let user_router = new_user_router(UserRouterParams {
        secret: basic_config.secret.clone(),
        magic_code,
        pool: get_db_pool(),
        cache,
    });
    let file_router = new_file_router(FileRouterParams {
        storage: get_opendal_storage(),
        pool: get_db_pool(),
    });
    let schema_router = new_schema_router();

    Ok(Router::new()
        .nest("/users", user_router)
        .nest("/files", file_router)
        .nest("/schemas", schema_router)
        .merge(common_router))
}
