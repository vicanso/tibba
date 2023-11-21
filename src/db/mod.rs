use serde::{Deserialize, Serialize};

mod conn;
mod settings;
mod users;

#[derive(Debug, Deserialize)]
pub struct FindRecordParams {
    pub table: String,
    pub orders: Option<String>,
    pub page: u64,
    pub page_size: u64,
}

#[derive(Debug, Eq, PartialEq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum EntityItemCategory {
    #[default]
    Text,
    Number,
    DateTime,
    Editor,
}

#[derive(Debug, Serialize, Default)]
pub struct EntityItemOption {
    pub label: String,
    pub str_value: Option<String>,
    pub num_value: Option<i32>,
}

#[derive(Debug, Serialize, Default)]
pub struct EntityItemDescription {
    pub name: String,
    pub category: EntityItemCategory,
    pub readonly: bool,
    pub options: Option<Vec<EntityItemOption>>,
}

pub use conn::get_database;
pub use settings::*;
pub use users::*;
