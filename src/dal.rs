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

use crate::config::must_get_config;
use ctor::ctor;
use once_cell::sync::OnceCell;
use tibba_error::new_error;
use tibba_hook::register_before_task;
use tibba_opendal::{Storage, new_opendal_storage};
use tracing::info;

static OPENDAL_STORAGE: OnceCell<Storage> = OnceCell::new();

pub fn get_opendal_storage() -> &'static Storage {
    // init opendal storage is checked in init function
    OPENDAL_STORAGE.get().unwrap()
}

#[ctor]
fn init() {
    register_before_task(
        "init_opendal_storage",
        16,
        Box::new(|| {
            Box::pin(async {
                let app_config = must_get_config();
                let storage = new_opendal_storage(&app_config.sub_config("opendal"))?;
                let info = storage.info();
                OPENDAL_STORAGE
                    .set(storage)
                    .map_err(|_| new_error("set opendal storage fail"))?;

                info!(
                    schema = ?info.scheme(),
                    full_capability = ?info.full_capability(),
                    "open dal storage init success"
                );

                Ok(())
            })
        }),
    );
}
