//! `SeaORM` Entity, @generated by sea-orm-codegen 1.0.0

use super::constants::Status;
use async_trait::async_trait;
use chrono::Utc;
use sea_orm::entity::prelude::*;
use sea_orm::ActiveValue;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub status: i8,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
    #[sea_orm(unique)]
    pub account: String,
    pub password: String,
    pub roles: Option<Json>,
    pub groups: Option<Json>,
    pub remark: Option<String>,
    pub email: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl ActiveModel {
    fn validate(&self) -> Result<(), DbErr> {
        if self.account.is_not_set() {
            return Err(DbErr::Custom("Account is required".to_string()));
        }
        if self.password.is_not_set() {
            return Err(DbErr::Custom("Password is required".to_string()));
        }
        Ok(())
    }
}

#[async_trait]
impl ActiveModelBehavior for ActiveModel {
    async fn before_save<C: ConnectionTrait>(
        mut self,
        _db: &C,
        insert: bool,
    ) -> Result<Self, DbErr> {
        if insert {
            self.validate()?;
            if self.status.is_not_set() {
                self.status = ActiveValue::set(Status::Enabled.to_value());
            }
            self.created_at = ActiveValue::set(Utc::now());
        }
        self.updated_at = ActiveValue::set(Utc::now());
        Ok(self)
    }
    async fn before_delete<C: ConnectionTrait>(self, _db: &C) -> Result<Self, DbErr> {
        // 禁止删除数据
        Err(DbErr::Custom("Delete is forbidden".to_string()))
    }
}
