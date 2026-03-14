use sea_orm::DatabaseConnection;

pub struct ServiceRepository<'a> {
    db: &'a DatabaseConnection,
}

impl<'a> ServiceRepository<'a> {
    pub fn new(db: &'a DatabaseConnection) -> Self {
        Self { db }
    }
}
