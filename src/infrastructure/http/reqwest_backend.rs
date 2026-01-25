use crate::domain::models::http_models::{HttpClientError, HttpEndpoint, HttpMethod, HttpResponse};
use crate::domain::traits::cookie_traits::CookieStore;
use crate::domain::traits::http_traits::{DecryptionProvider, EncryptionProvider, HttpClient};
use crate::infrastructure::http::cookie_backend::FileBackedCookieStore;
use crate::service::config::{CookieConfig, HttpConfig};
use async_trait::async_trait;
use reqwest::{Client, Method, Url};
use std::sync::Arc;
use std::time::Duration;
use crate::domain::models::cookie_models::{Cookie, SameSite};

pub struct ReqwestBackend {
    encryption_provider: Option<Arc<dyn EncryptionProvider>>,
    decryption_provider: Option<Arc<dyn DecryptionProvider>>,
    cookie_store: Option<Arc<dyn CookieStore>>,
    auto_save_handle: Option<tokio::task::JoinHandle<()>>,
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
            cookie_store: None,
            auto_save_handle: None,
            client,
        })
    }

    pub fn with_parameters(
        config: HttpConfig,
        cookie_store: Option<Arc<dyn CookieStore>>,
        auto_save_handle: Option<tokio::task::JoinHandle<()>>
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
            cookie_store,
            auto_save_handle,
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

impl ReqwestBackend {
    async fn inject_cookies(
        &self,
        url: &str,
        request_builder: reqwest::RequestBuilder,
        cookie_store: &Arc<dyn CookieStore>,
    ) -> Result<reqwest::RequestBuilder, HttpClientError> {
        let cookies = cookie_store.get_for_url(url).await;
        if cookies.is_empty() {
            return Ok(request_builder);
        }

        let cookie_header: String = cookies
            .iter()
            .map(|c| format!("{}={}", c.key.name, c.value))
            .collect::<Vec<_>>()
            .join("; ");

        Ok(request_builder.header(
            reqwest::header::COOKIE,
            reqwest::header::HeaderValue::from_str(&cookie_header)
                .map_err(|e| HttpClientError::InvalidHeader(e.to_string()))?,
        ))
    }

    async fn extract_cookies(
        &self,
        response: &reqwest::Response,
        cookie_store: &Arc<dyn CookieStore>,
    ) -> Result<(), HttpClientError> {
        if let Some(url) = response.url().host_str() {
            for cookie in response.cookies() {
                let name = cookie.name();
                let value = cookie.value();
                
                let first_same_site = match cookie.same_site_lax() {
                    true => {
                        SameSite::Lax
                    }
                    false => {
                        SameSite::Strict
                    }
                };
                let second_same_site = match cookie.same_site_strict() {
                    true => {
                        SameSite::Strict
                    }
                    false => {
                        SameSite::Lax
                    }
                };

                let same_site = if (first_same_site != second_same_site) {
                    None
                } else {
                    Some(first_same_site)
                };
                
                let cookie = Cookie::new(
                    url.to_string(),
                    response.url().path().to_string(),
                    name.to_string(),
                    value.to_string(),
                    cookie.expires(),
                    cookie.secure(),
                    cookie.http_only(),
                    same_site,
                );

                cookie_store.set(cookie).await;
            }
        }

        Ok(())
    }
}

#[async_trait]
impl HttpClient for ReqwestBackend {
    fn set_encryption_provider(&mut self, encryption_provider: Arc<dyn EncryptionProvider>) {
        self.encryption_provider = Some(encryption_provider);
    }

    fn set_decryption_provider(&mut self, decryption_provider: Arc<dyn DecryptionProvider>) {
        self.decryption_provider = Some(decryption_provider);
    }

    fn remove_encryption_provider(&mut self) -> Option<Arc<dyn EncryptionProvider>> {
        self.encryption_provider.take()
    }

    fn remove_decryption_provider(&mut self) -> Option<Arc<dyn DecryptionProvider>> {
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
        let url = endpoint.build_url();
        let parsed_url =
            Url::parse(&url).map_err(|e| HttpClientError::InvalidUrl(e.to_string()))?;
        let mut request_builder = self.client.request(method, &url);

        if endpoint.headers.is_some() {
            let headers = endpoint.headers.unwrap();
            for (key, value) in headers {
                request_builder = request_builder.header(&key, value);
            }
        }

        if endpoint.user_agent.is_some() {
            let user_agent = endpoint.user_agent.unwrap();
            request_builder = request_builder.header(reqwest::header::USER_AGENT, user_agent);
        }

        if endpoint.content_type.is_some() {
            let content_type = endpoint.content_type.unwrap();
            request_builder = request_builder.header(reqwest::header::CONTENT_TYPE, content_type);
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

        if let Some(cookie_store) = &self.cookie_store {
            request_builder = self
                .inject_cookies(&url, request_builder, cookie_store)
                .await?;
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

        if let Some(cookie_store) = &self.cookie_store {
            let _ = self.extract_cookies(&response, cookie_store).await;
        }

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
