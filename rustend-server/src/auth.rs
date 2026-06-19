use std::{
    future::Future,
    net::{IpAddr, SocketAddr},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use async_trait::async_trait;
use axum::{
    extract::ConnectInfo,
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
};
use tower::{Layer, Service};
use rustend_core::{ClientId, UserId};

#[derive(Debug, Clone)]
pub struct AuthInfo {
    pub client_id: ClientId,
    pub user_id:   UserId,
    pub roles:     Vec<String>,
}

#[derive(Debug)]
pub enum AuthError {
    Unauthenticated,
    Internal(String),
}

#[async_trait]
pub trait AuthProvider: Send + Sync + 'static {
    async fn authenticate(&self, ip: IpAddr) -> Result<AuthInfo, AuthError>;
}

#[derive(Clone, Copy)]
pub(crate) enum IpSource {
    ConnectInfo,
    ForwardedFor,
    RealIp,
}

#[derive(Clone)]
pub(crate) struct AuthLayer {
    provider:  Arc<dyn AuthProvider>,
    pool:      sqlx::PgPool,
    ip_source: IpSource,
}

impl AuthLayer {
    pub fn new(
        provider:  Arc<dyn AuthProvider>,
        pool:      sqlx::PgPool,
        ip_source: IpSource,
    ) -> Self {
        Self { provider, pool, ip_source }
    }
}

impl<S> Layer<S> for AuthLayer {
    type Service = AuthService<S>;
    fn layer(&self, inner: S) -> Self::Service {
        AuthService {
            inner,
            provider:  self.provider.clone(),
            pool:      self.pool.clone(),
            ip_source: self.ip_source,
        }
    }
}

#[derive(Clone)]
pub(crate) struct AuthService<S> {
    inner:     S,
    provider:  Arc<dyn AuthProvider>,
    pool:      sqlx::PgPool,
    ip_source: IpSource,
}

impl<S, B> Service<Request<B>> for AuthService<S>
where
    S: Service<Request<B>, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
    B: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Response, S::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), S::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<B>) -> Self::Future {
        let provider  = self.provider.clone();
        let pool      = self.pool.clone();
        let ip_source = self.ip_source;
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let ip = extract_ip(&req, ip_source);
            let ip = match ip {
                Some(ip) => ip,
                None     => return Ok(unauthenticated()),
            };

            let auth_info = match provider.authenticate(ip).await {
                Ok(info)                    => info,
                Err(AuthError::Unauthenticated) => return Ok(unauthenticated()),
                Err(AuthError::Internal(_)) => return Ok(provider_error()),
            };

            if let Err(e) = crate::db::clients::upsert_client(
                &pool, auth_info.client_id, auth_info.user_id,
            ).await {
                tracing::error!("auth: failed to upsert client: {e}");
                return Ok(provider_error());
            }

            req.extensions_mut().insert(auth_info);
            inner.call(req).await
        })
    }
}

fn extract_ip<B>(req: &Request<B>, ip_source: IpSource) -> Option<IpAddr> {
    match ip_source {
        IpSource::ConnectInfo => {
            let ip = req
                .extensions()
                .get::<ConnectInfo<SocketAddr>>()
                .map(|ci| ci.0.ip());
            #[cfg(test)]
            let ip = ip.or_else(|| {
                use axum::extract::connect_info::MockConnectInfo;
                req.extensions()
                    .get::<MockConnectInfo<SocketAddr>>()
                    .map(|m| m.0.ip())
            });
            ip
        }
        IpSource::ForwardedFor => req
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split(',').next())
            .and_then(|s| s.trim().parse::<IpAddr>().ok()),
        IpSource::RealIp => req
            .headers()
            .get("x-real-ip")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.trim().parse::<IpAddr>().ok()),
    }
}

fn unauthenticated() -> Response {
    (StatusCode::UNAUTHORIZED,
     axum::Json(serde_json::json!({"error": "unauthenticated"}))).into_response()
}

fn provider_error() -> Response {
    (StatusCode::INTERNAL_SERVER_ERROR,
     axum::Json(serde_json::json!({"error": "internal server error"}))).into_response()
}
