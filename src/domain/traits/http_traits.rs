use std::sync::Arc;
use async_trait::async_trait;
use crate::domain::models::http_models::{HttpClientError, HttpEndpoint, HttpResponse};

#[async_trait]
pub trait HttpClient: Send + Sync + 'static {
    fn set_encryption_provider(&mut self, encryption_provider: Arc<dyn EncryptionProvider>);
    fn set_decryption_provider(&mut self, decryption_provider: Arc<dyn DecryptionProvider>);

    fn remove_encryption_provider(&mut self) -> Option<Arc<dyn EncryptionProvider>>;
    fn remove_decryption_provider(&mut self) -> Option<Arc<dyn DecryptionProvider>>;

    async fn execute(&self, endpoint: HttpEndpoint) -> Result<HttpResponse, HttpClientError>;
}

#[async_trait]
pub trait EncryptionProvider: Send + Sync + 'static {
    async fn encrypt(&self, bytes: &mut Vec<u8>) -> Result<Vec<u8>, HttpClientError>;
}

#[async_trait]
pub trait DecryptionProvider: Send + Sync + 'static {
    async fn decrypt(&self, bytes: &mut Vec<u8>) -> Result<Vec<u8>, HttpClientError>;
}
