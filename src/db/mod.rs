use crate::error::HttpError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type Result<T, E = HttpError> = std::result::Result<T, E>;

mod conn;
mod settings;
mod users;

#[derive(Debug, Deserialize, Clone)]
pub struct ListCountParams {
    pub table: String,
    pub orders: Option<String>,
    pub keyword: Option<String>,
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
    Status,
    Json,
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
    pub label: String,
    pub category: EntityItemCategory,
    pub readonly: bool,
    pub options: Option<Vec<EntityItemOption>>,
    pub width: Option<i16>,
}

const TABLE_NAME_SETTINGS: &str = "settings";
const TABLE_NAME_USERS: &str = "users";
const TABLE_INVALID_MSG: &str = "Table is invalid";

pub async fn list_count(params: &ListCountParams) -> Result<(i64, Vec<Value>)> {
    let result = match params.table.as_str() {
        TABLE_NAME_SETTINGS => SettingEntity::list_count(params).await?,
        TABLE_NAME_USERS => UserEntity::list_count(params).await?,
        _ => return Err(HttpError::new(TABLE_INVALID_MSG)),
    };
    Ok(result)
}
pub fn list_descriptions(name: &str) -> Result<Vec<EntityItemDescription>> {
    let result = match name {
        TABLE_NAME_SETTINGS => SettingEntity::list_descriptions(),
        TABLE_NAME_USERS => UserEntity::list_descriptions(),
        _ => return Err(HttpError::new(TABLE_INVALID_MSG)),
    };
    Ok(result)
}
pub async fn add(name: &str, user: &str, value: &Value) -> Result<i64> {
    let id = match name {
        TABLE_NAME_SETTINGS => {
            let result = SettingEntity::insert(user, value).await?;
            result.id
        }
        _ => return Err(HttpError::new(TABLE_INVALID_MSG)),
    };
    Ok(id)
}

pub use conn::get_database;
pub use settings::*;
pub use users::*;
