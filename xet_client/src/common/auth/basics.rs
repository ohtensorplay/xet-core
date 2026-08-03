use std::sync::Arc;

use async_trait::async_trait;
use reqwest_middleware::RequestBuilder;

use super::CredentialHelper;
use crate::error::ClientError;

pub struct NoopCredentialHelper {}

impl NoopCredentialHelper {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {})
    }
}

#[async_trait]
impl CredentialHelper for NoopCredentialHelper {
    async fn fill_credential(&self, req: RequestBuilder) -> Result<RequestBuilder, ClientError> {
        Ok(req)
    }

    fn whoami(&self) -> &str {
        "noop"
    }
}

pub struct BearerCredentialHelper {
    pub token: String,

    _whoami: &'static str,
}

impl BearerCredentialHelper {
    pub fn new(token: String, whoami: &'static str) -> Arc<Self> {
        Arc::new(Self { token, _whoami: whoami })
    }
}

#[async_trait]
impl CredentialHelper for BearerCredentialHelper {
    async fn fill_credential(&self, req: RequestBuilder) -> Result<RequestBuilder, ClientError> {
        Ok(req.bearer_auth(&self.token))
    }

    fn whoami(&self) -> &str {
        self._whoami
    }
}
