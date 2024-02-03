use crate::error::HttpError;
use async_trait::async_trait;
use sea_orm::{ColumnTrait, Condition};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use snafu::Snafu;

pub use conn::get_database;
pub use files::*;
pub use settings::*;
pub use users::*;

pub type Result<T, E = HttpError> = std::result::Result<T, E>;

pub static ROLE_SU: &str = "su";
pub static ROLE_ADMIN: &str = "admin";

mod conn;
mod files;
mod settings;
mod users;

#[async_trait]
pub trait CommonEntity {
    async fn validate_for_update(_user: &str) -> Result<()> {
        Ok(())
    }
    async fn validate_for_insert(_user: &str) -> Result<()> {
        Ok(())
    }
    async fn validate_for_query(_user: &str) -> Result<()> {
        Ok(())
    }
    fn get_condition(_params: &ListCountParams) -> Option<Condition> {
        None
    }
    fn get_columns() -> Option<Vec<String>> {
        None
    }
}

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Record not found"))]
    NotFound,
    #[snafu(display("Order by {order} is unsupported"))]
    OrderNotSupport { order: String },
}

impl From<Error> for HttpError {
    fn from(value: Error) -> Self {
        HttpError::new_with_category(&value.to_string(), "db")
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct ListCountParams {
    // pub table: String,
    pub orders: Option<String>,
    pub keyword: Option<String>,
    pub page: u64,
    pub page_size: u64,
    pub counted: bool,
}

impl ListCountParams {
    pub fn validate(&self) -> Result<()> {
        if self.page_size == 0 {
            return Err(HttpError::new("每页记录数不能为0"));
        }
        Ok(())
    }
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
    Texts,
    Number,
    DateTime,
    Editor,
    Status,
    Json,
    File,
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
    pub width: Option<u16>,
    pub span: Option<u8>,
}

const TABLE_NAME_SETTINGS: &str = "settings";
const TABLE_NAME_USERS: &str = "users";
const TABLE_NAME_FILES: &str = "files";
const TABLE_INVALID_MSG: &str = "Table is invalid";

pub async fn list_count(
    name: &str,
    user: &str,
    params: &ListCountParams,
) -> Result<(i64, Vec<Value>)> {
    let result = match name {
        TABLE_NAME_SETTINGS => SettingEntity::list_count(user, params).await?,
        TABLE_NAME_FILES => FileEntity::list_count(user, params).await?,
        TABLE_NAME_USERS => UserEntity::list_count(user, params).await?,
        _ => return Err(HttpError::new(TABLE_INVALID_MSG)),
    };
    Ok(result)
}
pub fn description(name: &str) -> Result<EntityDescription> {
    let result = match name {
        TABLE_NAME_SETTINGS => SettingEntity::description(),
        TABLE_NAME_USERS => UserEntity::description(),
        TABLE_NAME_FILES => FileEntity::description(),
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
        TABLE_NAME_FILES => {
            let result = FileEntity::insert(user, value).await?;
            result.id
        }
        _ => return Err(HttpError::new(TABLE_INVALID_MSG)),
    };
    Ok(id)
}
pub async fn find_by_id(name: &str, user: &str, id: i64) -> Result<Option<Value>> {
    let result = match name {
        TABLE_NAME_SETTINGS => SettingEntity::find_by_id(user, id).await?,
        TABLE_NAME_USERS => UserEntity::find_by_id(user, id).await?,
        TABLE_NAME_FILES => FileEntity::find_by_id(user, id).await?,
        _ => return Err(HttpError::new(TABLE_INVALID_MSG)),
    };
    Ok(result)
}
pub async fn update_by_id(name: &str, user: &str, id: i64, value: &Value) -> Result<()> {
    match name {
        TABLE_NAME_SETTINGS => SettingEntity::update_by_id(user, id, value).await?,
        TABLE_NAME_USERS => UserEntity::update_by_id(user, id, value).await?,
        TABLE_NAME_FILES => FileEntity::update_by_id(user, id, value).await?,
        _ => return Err(HttpError::new(TABLE_INVALID_MSG)),
    };

    Ok(())
}
