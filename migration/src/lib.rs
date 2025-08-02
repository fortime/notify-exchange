pub use sea_orm_migration::prelude::*;

mod m20220101_000001_create_table;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(m20220101_000001_create_table::Migration)]
    }
}

trait ColumnDefExt {
    fn case_sensitive(self: &mut Self, manager: &SchemaManager<'_>) -> &mut Self;
}

impl ColumnDefExt for ColumnDef {
    fn case_sensitive(self: &mut Self, manager: &SchemaManager<'_>) -> &mut Self {
        match manager.get_database_backend() {
            sea_orm::DatabaseBackend::MySql => {
                self.extra("CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_bin")
            },
            _ => self
        }
    }
}
