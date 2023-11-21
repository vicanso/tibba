use super::{get_database, EntityItemCategory, EntityItemDescription, FindRecordParams};
use crate::entities::settings::{ActiveModel, Column, Entity, Model};
use crate::error::HttpError;
use crate::util::{json_get_date_time, json_get_i64, json_get_string};
use sea_orm::query::{Order, Select};
use sea_orm::{entity::prelude::*, ActiveValue, QueryOrder};
use serde_json::Value;
use substring::Substring;

pub type Result<T, E = HttpError> = std::result::Result<T, E>;

const ERROR_CATEGORY: &str = "entity_settings";

fn from_value(value: Value) -> Result<ActiveModel> {
    let mut model = ActiveModel {
        ..Default::default()
    };
    if let Some(id) = json_get_i64(&value, "id")? {
        model.id = ActiveValue::set(id);
    }
    if let Some(status) = json_get_i64(&value, "status")? {
        model.status = ActiveValue::set(status as i8);
    }
    if let Some(name) = json_get_string(&value, "name")? {
        model.name = ActiveValue::set(name);
    }
    if let Some(category) = json_get_string(&value, "category")? {
        model.category = ActiveValue::set(category);
    }
    if let Some(data) = json_get_string(&value, "data")? {
        model.data = ActiveValue::set(data);
    }
    if let Some(remark) = json_get_string(&value, "remark")? {
        model.remark = ActiveValue::set(remark);
    }
    if let Some(started_at) = json_get_date_time(&value, "started_at")? {
        model.started_at = ActiveValue::set(started_at);
    }
    if let Some(ended_at) = json_get_date_time(&value, "ended_at")? {
        model.ended_at = ActiveValue::set(ended_at);
    }
    Ok(model)
}

pub fn order_by<E>(sql: Select<E>, orders: &str) -> Result<Select<E>>
where
    E: EntityTrait,
{
    if orders.is_empty() {
        return Ok(sql);
    }
    let mut s = sql;
    // TODO 是否有办法字符串转换为column
    for order in orders.split(',') {
        let mut order_type = Order::Asc;
        let key = if order.starts_with('-') {
            order_type = Order::Desc;
            order.substring(1, order.len())
        } else {
            order
        };

        match key {
            "id" => s = s.order_by(Column::Id, order_type),
            "updated_at" => s = s.order_by(Column::UpdatedAt, order_type),
            "status" => s = s.order_by(Column::Status, order_type),
            _ => {
                let msg = format!("Order by {key} is unsupported");
                return Err(HttpError::new_with_category(&msg, ERROR_CATEGORY));
            }
        }
    }
    Ok(s)
}

pub async fn add_setting_json(user: &str, value: Value) -> Result<Model> {
    let mut data = from_value(value)?;
    data.creator = ActiveValue::set(user.to_string());
    let result = data.insert(get_database().await).await?;
    Ok(result)
}

pub fn list_setting_descriptions() -> Vec<EntityItemDescription> {
    vec![
        EntityItemDescription {
            name: Column::Id.to_string(),
            category: EntityItemCategory::Number,
            readonly: true,
            ..Default::default()
        },
        EntityItemDescription {
            name: Column::Status.to_string(),
            category: EntityItemCategory::Number,
            ..Default::default()
        },
        EntityItemDescription {
            name: Column::Name.to_string(),
            category: EntityItemCategory::Text,
            ..Default::default()
        },
        EntityItemDescription {
            name: Column::Category.to_string(),
            category: EntityItemCategory::Text,
            ..Default::default()
        },
        EntityItemDescription {
            name: Column::Data.to_string(),
            category: EntityItemCategory::Editor,
            ..Default::default()
        },
        EntityItemDescription {
            name: Column::Remark.to_string(),
            category: EntityItemCategory::Text,
            ..Default::default()
        },
        EntityItemDescription {
            name: Column::StartedAt.to_string(),
            category: EntityItemCategory::DateTime,
            ..Default::default()
        },
        EntityItemDescription {
            name: Column::EndedAt.to_string(),
            category: EntityItemCategory::DateTime,
            ..Default::default()
        },
        EntityItemDescription {
            name: Column::CreatedAt.to_string(),
            category: EntityItemCategory::DateTime,
            readonly: true,
            ..Default::default()
        },
        EntityItemDescription {
            name: Column::UpdatedAt.to_string(),
            category: EntityItemCategory::DateTime,
            readonly: true,
            ..Default::default()
        },
        EntityItemDescription {
            name: Column::Creator.to_string(),
            category: EntityItemCategory::Text,
            readonly: true,
            ..Default::default()
        },
    ]
}

pub async fn find_count_setting_json(params: FindRecordParams) -> Result<(i64, Vec<Value>)> {
    let conn = get_database().await;
    let page_count = if params.page == 0 {
        let count = Entity::find().count(conn).await?;
        let mut page_count = count / params.page_size;
        if count % params.page_size != 0 {
            page_count += 1;
        }
        page_count as i64
    } else {
        -1
    };

    let mut sql = Entity::find();
    sql = order_by(sql, &params.orders.unwrap_or("-updated_at".to_string()))?;
    let items = sql
        .into_json()
        .paginate(conn, params.page_size)
        .fetch_page(params.page)
        .await?;

    Ok((page_count, items))
}
