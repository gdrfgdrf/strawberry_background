
use async_trait::async_trait;
use crate::domain::models::{HttpClientError, HttpEndpoint, HttpResponse};

#[async_trait]
pub trait HttpClient: Send + Sync + 'static {
    fn set_encryption_provider(&mut self, encryption_provider: Box<dyn EncryptionProvider>);
    fn set_decryption_provider(&mut self, decryption_provider: Box<dyn DecryptionProvider>);

    fn remove_encryption_provider(&mut self) -> Option<Box<dyn EncryptionProvider>>;
    fn remove_decryption_provider(&mut self) -> Option<Box<dyn DecryptionProvider>>;

    async fn execute(&self, endpoint: HttpEndpoint) -> Result<HttpResponse, HttpClientError>;
}

#[async_trait]
pub trait EncryptionProvider: Send + Sync + 'static {
    async fn encrypt(&self, bytes: Vec<u8>) -> Vec<u8>;
}

#[async_trait]
pub trait DecryptionProvider: Send + Sync + 'static {
    async fn decrypt(&self, bytes: Vec<u8>) -> Vec<u8>;
}