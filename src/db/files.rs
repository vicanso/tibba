use super::{
    get_database, EntityDescription, EntityItemCategory, EntityItemDescription, Error,
    ListCountParams, Result, ROLE_SU,
};
use crate::entities::files::{ActiveModel, Column, Entity, Model};
use crate::util::{json_get_date_time, json_get_i64, json_get_string};
use once_cell::sync::Lazy;
use sea_orm::query::{Order, Select};
use sea_orm::Condition;
use sea_orm::{entity::prelude::*, ActiveValue::Set, QueryOrder};
use serde_json::Value;
use substring::Substring;

fn from_value(value: &Value) -> Result<ActiveModel> {
    let mut model = ActiveModel{
        ..Default::default()
    };
    if let Some(id) = json_get_i64(value, Column::Id.as_str())?  {
        model.id = Set(id); 
    }
    if let Some(name) = json_get_string(value, Column::Name.as_str())? {
        model.name = Set(name);
    }
    if let Some(content_type) = json_get_string(value, Column::ContentType.as_str())? {
        model.content_type = Set(content_type); 
    }
    

    Ok(model)
}