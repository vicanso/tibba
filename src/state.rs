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

use super::config::must_get_basic_config;
use once_cell::sync::OnceCell;
use tibba_state::AppState;

static STATE: OnceCell<AppState> = OnceCell::new();

pub fn get_app_state() -> &'static AppState {
    STATE.get_or_init(|| {
        let basic_config = must_get_basic_config();
        AppState::new(
            basic_config.processing_limit,
            basic_config.commit_id.clone(),
        )
    })
}
