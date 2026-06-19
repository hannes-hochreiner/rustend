use std::sync::Arc;
use sqlx::PgPool;
use crate::auth::{AuthProvider, IpSource};

#[derive(Clone)]
pub struct ServerStore {
    pub pool:      PgPool,
    pub(crate) auth:      Arc<dyn AuthProvider>,
    pub(crate) ip_source: IpSource,
}

impl ServerStore {
    pub fn new(pool: PgPool, auth: impl AuthProvider) -> Self {
        Self {
            pool,
            auth:      Arc::new(auth),
            ip_source: IpSource::ConnectInfo,
        }
    }

    pub fn trust_forwarded_for(mut self) -> Self {
        self.ip_source = IpSource::ForwardedFor;
        self
    }

    pub fn trust_real_ip(mut self) -> Self {
        self.ip_source = IpSource::RealIp;
        self
    }
}
