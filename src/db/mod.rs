use crate::error::HttpError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type Result<T, E = HttpError> = std::result::Result<T, E>;

pub static ROLE_SU: &str = "su";

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

#[derive(Debug, Serialize, Default)]
pub struct EntityDescription {
    pub items: Vec<EntityItemDescription>,
    pub support_orders: Vec<String>,
    pub modify_roles: Vec<String>,
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

pub async fn list_count(user: &str, params: &ListCountParams) -> Result<(i64, Vec<Value>)> {
    let result = match params.table.as_str() {
        TABLE_NAME_SETTINGS => SettingEntity::list_count(user, params).await?,
        TABLE_NAME_USERS => UserEntity::list_count(user, params).await?,
        _ => return Err(HttpError::new(TABLE_INVALID_MSG)),
    };
    Ok(result)
}
pub fn description(name: &str) -> Result<EntityDescription> {
    let result = match name {
        TABLE_NAME_SETTINGS => SettingEntity::description(),
        TABLE_NAME_USERS => UserEntity::description(),
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
