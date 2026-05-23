use sqlx::PgPool;

#[derive(Clone)]
pub struct ServerStore {
    pub pool: PgPool,
}

impl ServerStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}
