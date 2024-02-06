use super::CommonEntity;
use super::{
    get_database, EntityDescription, EntityItemCategory, EntityItemDescription, EntityItemOption,
    Error, ListCountParams, Result, ROLE_SU,
};
use crate::entities::settings::{ActiveModel, Column, Entity, Model};
use crate::util::{json_get_date_time, json_get_i64, json_get_string};
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

static SUPPORT_ORDERS: Lazy<Vec<Column>> = Lazy::new(|| {
    vec![
        Column::StartedAt,
        Column::EndedAt,
        Column::UpdatedAt,
        Column::Status,
    ]
});

#[derive(DbEntity)]
pub struct SettingEntity {}
impl CommonEntity for SettingEntity {}

impl SettingEntity {
    fn get_support_orders() -> Vec<Column> {
        SUPPORT_ORDERS.to_vec()
    }
    fn update_from_value(model: &mut ActiveModel, value: &Value) -> Result<()> {
        if let Some(status) = json_get_i64(value, Column::Status.as_str())? {
            model.status = Set(status as i8);
        }
        if let Some(name) = json_get_string(value, Column::Name.as_str())? {
            model.name = Set(name);
        }
        if let Some(category) = json_get_string(value, Column::Category.as_str())? {
            model.category = Set(category);
        }
        if let Some(data) = json_get_string(value, Column::Data.as_str())? {
            model.data = Set(data);
        }
        if let Some(remark) = json_get_string(value, Column::Remark.as_str())? {
            model.remark = Set(remark);
        }
        if let Some(started_at) = json_get_date_time(value, Column::StartedAt.as_str())? {
            model.started_at = Set(started_at);
        }
        if let Some(ended_at) = json_get_date_time(value, Column::EndedAt.as_str())? {
            model.ended_at = Set(ended_at);
        }
        Ok(())
    }
    fn get_condition(params: &ListCountParams) -> Option<Condition> {
        if let Some(keyword) = &params.keyword {
            let cond = Condition::any()
                .add(Column::Category.eq(keyword))
                .add(Column::Name.contains(keyword))
                .add(Column::Remark.contains(keyword));
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
                name: Column::Status.to_string(),
                label: "状态".to_string(),
                width: Some(90),
                category: EntityItemCategory::Status,
                ..Default::default()
            },
            EntityItemDescription {
                name: Column::Category.to_string(),
                label: "分类".to_string(),
                width: Some(80),
                category: EntityItemCategory::Text,
                options: Some(vec![
                    EntityItemOption {
                        label: "应用配置".to_string(),
                        str_value: Some("system".to_string()),
                        ..Default::default()
                    },
                    EntityItemOption {
                        label: "业务配置".to_string(),
                        str_value: Some("biz".to_string()),
                        ..Default::default()
                    },
                ]),
                ..Default::default()
            },
            EntityItemDescription {
                name: Column::StartedAt.to_string(),
                label: "生效时间".to_string(),
                width: Some(150),
                category: EntityItemCategory::DateTime,
                ..Default::default()
            },
            EntityItemDescription {
                name: Column::EndedAt.to_string(),
                label: "失效时间".to_string(),
                width: Some(150),
                category: EntityItemCategory::DateTime,
                ..Default::default()
            },
            EntityItemDescription {
                name: Column::Creator.to_string(),
                label: "创建人".to_string(),
                width: Some(80),
                category: EntityItemCategory::Text,
                readonly: true,
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
                name: Column::Remark.to_string(),
                label: "备注".to_string(),
                width: Some(120),
                span: Some(3),
                category: EntityItemCategory::Text,
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
