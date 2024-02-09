use super::CommonEntity;
use super::{
    get_database, EntityDescription, EntityItemCategory, EntityItemDescription, Error,
    ListCountParams, Result, ROLE_SU,
};
use crate::entities::files::{ActiveModel, Column, Entity, Model};
use crate::util::{json_get_bytes, json_get_string};
use db_entity_derive::DbEntity;
use once_cell::sync::Lazy;
use sea_orm::query::{Order, Select};
use sea_orm::ColumnTrait;
use sea_orm::Condition;
use sea_orm::QuerySelect;
use sea_orm::{entity::prelude::*, ActiveValue::Set, QueryOrder};
use serde_json::Value;
use std::str::FromStr;
use substring::Substring;

static SUPPORT_ORDERS: Lazy<Vec<Column>> = Lazy::new(|| vec![Column::Id, Column::Name]);

#[derive(DbEntity)]
pub struct FileEntity {}
impl CommonEntity for FileEntity {}

impl FileEntity {
    fn get_support_orders() -> Vec<Column> {
        SUPPORT_ORDERS.to_vec()
    }
    fn update_from_value(model: &mut ActiveModel, value: &Value) -> Result<()> {
        if let Some(name) = json_get_string(value, Column::Name.as_str())? {
            model.name = Set(name);
        }
        if let Some(content_type) = json_get_string(value, Column::ContentType.as_str())? {
            model.content_type = Set(content_type);
        }
        if let Some(data) = json_get_bytes(value, Column::Data.as_str())? {
            model.data = Set(data)
        }

        Ok(())
    }
    fn get_condition(params: &ListCountParams) -> Option<Condition> {
        if let Some(keyword) = &params.keyword {
            let cond = Condition::any()
                .add(Column::Id.eq(keyword))
                .add(Column::Name.contains(keyword));
            Some(cond)
        } else {
            None
        }
    }
    pub fn description() -> EntityDescription {
        let items = vec![
            EntityItemDescription {
                name: Column::Id.to_string(),
                label: "ID".to_string(),
                auto_created: true,
                width: Some(60),
                category: EntityItemCategory::Number,
                readonly: true,
                ..Default::default()
            },
            EntityItemDescription {
                name: Column::Name.to_string(),
                label: "名称".to_string(),
                category: EntityItemCategory::Text,
                readonly: true,
                ..Default::default()
            },
            EntityItemDescription {
                name: Column::Creator.to_string(),
                label: "创建人".to_string(),
                width: Some(80),
                category: EntityItemCategory::Text,
                readonly: true,
                auto_created: true,
                ..Default::default()
            },
            EntityItemDescription {
                name: Column::Data.to_string(),
                label: "配置".to_string(),
                width: Some(200),
                category: EntityItemCategory::Editor,
                span: Some(3),
                ..Default::default()
            },
            EntityItemDescription {
                name: Column::Data.to_string(),
                label: "文件".to_string(),
                width: Some(120),
                span: Some(3),
                category: EntityItemCategory::File,
                ..Default::default()
            },
            EntityItemDescription {
                name: Column::CreatedAt.to_string(),
                label: "创建时间".to_string(),
                width: Some(150),
                category: EntityItemCategory::DateTime,
                readonly: true,
                ..Default::default()
            },
            EntityItemDescription {
                name: Column::UpdatedAt.to_string(),
                label: "更新时间".to_string(),
                width: Some(150),
                category: EntityItemCategory::DateTime,
                readonly: true,
                ..Default::default()
            },
        ];
        EntityDescription {
            items,
            modify_roles: vec![ROLE_SU.to_string()],
            support_orders: SUPPORT_ORDERS.iter().map(|item| item.to_string()).collect(),
            ..Default::default()
        }
    }
}
