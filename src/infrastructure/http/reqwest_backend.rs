use crate::domain::models::{HttpClientError, HttpEndpoint, HttpMethod, HttpResponse};
use crate::domain::traits::{DecryptionProvider, EncryptionProvider, HttpClient};
use crate::service::config::HttpConfig;
use async_trait::async_trait;
use reqwest::{Client, ClientBuilder, Method};
use std::time::Duration;

pub struct ReqwestBackend {
    encryption_provider: Option<Box<dyn EncryptionProvider>>,
    decryption_provider: Option<Box<dyn DecryptionProvider>>,
    client: Client,
}

impl ReqwestBackend {
    pub fn new() -> Result<Self, HttpClientError> {
        let client = Client::builder()
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(60))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| HttpClientError::Network(e.to_string()))?;
        Ok(Self {
            encryption_provider: None,
            decryption_provider: None,
            client,
        })
    }

    pub fn with_config(
        config: HttpConfig,
    ) -> Result<Self, HttpClientError> {
        let client = Client::builder()
            .pool_idle_timeout(config.pool_idle_timeout)
            .connect_timeout(config.connect_timeout)
            .timeout(config.request_timeout)
            .pool_max_idle_per_host(config.max_connections_per_host)
            .build()
            .map_err(|e| HttpClientError::Network(e.to_string()))?;
        Ok(Self {
            encryption_provider: config.encryption_provider,
            decryption_provider: config.decryption_provider,
            client,
        })
    }

    fn convert_method(method: &HttpMethod) -> Method {
        match method {
            HttpMethod::Get => Method::GET,
            HttpMethod::Post => Method::POST,
            HttpMethod::Put => Method::PUT,
            HttpMethod::Delete => Method::DELETE,
        }
    }
}

#[async_trait]
impl HttpClient for ReqwestBackend {
    fn set_encryption_provider(&mut self, encryption_provider: Box<dyn EncryptionProvider>) {
        self.encryption_provider = Some(encryption_provider);
    }

    fn set_decryption_provider(&mut self, decryption_provider: Box<dyn DecryptionProvider>) {
        self.decryption_provider = Some(decryption_provider);
    }

    fn remove_encryption_provider(&mut self) -> Option<Box<dyn EncryptionProvider>> {
        self.encryption_provider.take()
    }

    fn remove_decryption_provider(&mut self) -> Option<Box<dyn DecryptionProvider>> {
        self.decryption_provider.take()
    }

    async fn execute(&self, endpoint: HttpEndpoint) -> Result<HttpResponse, HttpClientError> {
        if endpoint.body.is_some()
            && endpoint.requires_encryption
            && self.encryption_provider.is_none()
        {
            return Err(HttpClientError::Configuration(
                "no encryption provider".parse().unwrap(),
            ));
        }
        if endpoint.body.is_some()
            && endpoint.requires_decryption
            && self.decryption_provider.is_none()
        {
            return Err(HttpClientError::Configuration(
                "no decryption provider".parse().unwrap(),
            ));
        }

        let method = Self::convert_method(&endpoint.method);
        let mut request_builder = self.client.request(method, &endpoint.build_url());

        if endpoint.headers.is_some() {
            let headers = endpoint.headers.unwrap();
            for (key, value) in headers {
                request_builder = request_builder.header(&key, value);
            }
        }

        if endpoint.user_agent.is_some() {
            let user_agent = endpoint.user_agent.unwrap();
            request_builder = request_builder.header("user-agent", user_agent);
        }

        if endpoint.content_type.is_some() {
            let content_type = endpoint.content_type.unwrap();
            request_builder = request_builder.header("content-type", content_type);
        }

        if endpoint.body.is_some() {
            let mut body = endpoint.body.unwrap();
            if endpoint.requires_encryption {
                body = self
                    .encryption_provider
                    .as_ref()
                    .unwrap()
                    .encrypt(body)
                    .await;
            }
            request_builder = request_builder.body(body);
        }

        let response = request_builder
            .timeout(endpoint.timeout)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    HttpClientError::Timeout(endpoint.timeout)
                } else {
                    HttpClientError::Network(e.to_string())
                }
            })?;

        let status = response.status().as_u16();
        let headers: Vec<(String, String)> = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();

        let mut body = response
            .bytes()
            .await
            .map_err(|e| HttpClientError::Network(e.to_string()))?
            .to_vec();

        if endpoint.requires_decryption {
            body = self
                .decryption_provider
                .as_ref()
                .unwrap()
                .decrypt(body)
                .await;
        }

        Ok(HttpResponse {
            status,
            headers,
            body, // 大文件应考虑使用流式处理，此处返回Vec<u8>作为示例
        })
    }
}
