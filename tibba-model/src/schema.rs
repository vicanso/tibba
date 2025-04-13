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

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SchemaType {
    #[default]
    String,
    Number,
    Bytes,
    Boolean,
    Status,
    Strings,
    Date,
    Json,
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
pub struct Schema {
    pub name: String,
    pub category: SchemaType,
    pub read_only: bool,
    pub required: bool,
    pub fixed: bool,
    pub options: Option<Vec<SchemaOption>>,
    pub hidden: bool,
    pub sortable: bool,
    pub filterable: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SchemaView {
    pub schemas: Vec<Schema>,
    // pub conditions: Vec<SchemaCondition>,
}
