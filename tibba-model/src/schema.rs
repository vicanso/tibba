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

use serde::{Deserialize, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Disabled = 0,
    Enabled = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultValue {
    Success = 0,
    Failed = 1,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SchemaType {
    #[default]
    String,
    Number,
    Bytes,
    Boolean,
    Status,
    Result,
    Strings,
    Date,
    ByteSize,
    Json,
    Code,
    HoverCard,
}

#[derive(Debug, Clone, Deserialize)]
pub enum SchemaOptionValue {
    String(String),
    Number(f64),
}

impl Serialize for SchemaOptionValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            SchemaOptionValue::String(s) => serializer.serialize_str(s),
            SchemaOptionValue::Number(n) => serializer.serialize_f64(*n),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SchemaOption {
    pub label: String,
    pub value: SchemaOptionValue,
}

pub(crate) fn new_schema_options(values: &[&str]) -> Vec<SchemaOption> {
    values
        .iter()
        .map(|v| SchemaOption {
            label: v.to_string(),
            value: SchemaOptionValue::String(v.to_string()),
        })
        .collect()
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SchemaConditionType {
    #[default]
    Input,
    Select,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SchemaCondition {
    pub name: String,
    pub category: SchemaConditionType,
    pub options: Option<Vec<SchemaOption>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SchemaAllowEdit {
    pub owner: bool,
    pub groups: Vec<String>,
    pub roles: Vec<String>,
    pub disabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SchemaAllowCreate {
    pub groups: Vec<String>,
    pub roles: Vec<String>,
    pub disabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Schema {
    pub name: String,
    pub label: Option<String>,
    pub category: SchemaType,
    pub identity: bool,
    pub read_only: bool,
    pub auto_create: bool,
    pub required: bool,
    pub fixed: bool,
    pub options: Option<Vec<SchemaOption>>,
    pub hidden: bool,
    pub popover: bool,
    pub sortable: bool,
    pub filterable: bool,
    pub span: Option<u8>,
    pub default_value: Option<serde_json::Value>,
    pub hidden_values: Vec<String>,
    pub max_width: Option<u16>,
    pub combinations: Option<Vec<String>>,
}

impl Schema {
    pub fn new_id() -> Self {
        Self {
            name: "id".to_string(),
            category: SchemaType::Number,
            read_only: true,
            required: true,
            hidden: true,
            auto_create: true,
            ..Default::default()
        }
    }
    pub fn new_status() -> Self {
        Self {
            name: "status".to_string(),
            category: SchemaType::Status,
            required: true,
            default_value: Some(serde_json::json!(Status::Enabled as i8)),
            ..Default::default()
        }
    }
    pub fn new_created() -> Self {
        Self {
            name: "created".to_string(),
            category: SchemaType::Date,
            read_only: true,
            hidden: true,
            auto_create: true,
            ..Default::default()
        }
    }
    pub fn new_modified() -> Self {
        Self {
            name: "modified".to_string(),
            category: SchemaType::Date,
            read_only: true,
            sortable: true,
            auto_create: true,
            ..Default::default()
        }
    }
    pub fn new_filterable_modified() -> Self {
        let mut modified = Self::new_modified();
        modified.filterable = true;
        modified
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SchemaView {
    pub schemas: Vec<Schema>,
    pub allow_edit: SchemaAllowEdit,
    pub allow_create: SchemaAllowCreate,
}
