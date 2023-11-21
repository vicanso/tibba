use sea_orm::entity::prelude::*;

#[derive(EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "i8", db_type = "Integer")]
pub enum Status {
    #[sea_orm(num_value = 0)]
    Disabled,
    #[sea_orm(num_value = 1)]
    Enabled,
}
