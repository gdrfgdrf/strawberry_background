use crate::db::models::cookies::SameSite;
use crate::db::models::preclude::{CookieKeysActiveModel, CookiesActiveModel};
use crate::domain::models::http_models::{
    HttpClientError, HttpEndpoint, HttpMethod, HttpResponse, HttpStreamResponse,
};
use crate::domain::traits::cookie_traits::CookieStore;
use crate::domain::traits::http_traits::{DecryptionProvider, EncryptionProvider, HttpClient};
use crate::infrastructure::http::cookie_backend::DatabaseCookieStore;
use crate::service::config::HttpConfig;
use crate::utils::progress_reader::AsyncProgressReader;
use crate::utils::stream_with_callback::StreamCallbackExt;
use chrono::{DateTime, FixedOffset, Utc};
use futures_util::TryStreamExt;
use reqwest::{Client, Method, Proxy, Response, Url};
use sea_orm::ActiveValue;
use std::io::ErrorKind;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio_util::compat::FuturesAsyncReadCompatExt;
use tracing::{Level, span};

pub struct ReqwestBackend {
    encryption_provider: Option<Arc<dyn EncryptionProvider>>,
    decryption_provider: Option<Arc<dyn DecryptionProvider>>,
    cookie_store: DatabaseCookieStore,
    client: Client,
    _session: span::Span,
}

impl ReqwestBackend {
    pub fn new(cookie_store: DatabaseCookieStore) -> Result<Self, HttpClientError> {
        let client = Client::builder()
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(60))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| HttpClientError::Network(e.to_string()))?;
        Ok(Self {
            encryption_provider: None,
            decryption_provider: None,
            cookie_store,
            client,
            _session: span!(Level::INFO, "reqwest-backend"),
        })
    }

    pub fn with_parameters(
        config: HttpConfig,
        cookie_store: DatabaseCookieStore,
    ) -> Result<Self, HttpClientError> {
        let _session = span!(Level::INFO, "reqwest-backend");
        let _ = _session.enter();

        tracing::debug!(
            pool_idle_timeout = ?config.pool_idle_timeout,
            connect_timeout = ?config.connect_timeout,
            request_timeout = ?config.request_timeout,
            max_connections = config.max_connections_per_host,
            "building HTTP client"
        );

        let mut client = Client::builder()
            .pool_idle_timeout(config.pool_idle_timeout)
            .connect_timeout(config.connect_timeout)
            .timeout(config.request_timeout)
            .tls_danger_accept_invalid_hostnames(config.tls_danger_accept_invalid_hostnames)
            .tls_danger_accept_invalid_certs(config.tls_danger_accept_invalid_certs)
            .pool_max_idle_per_host(config.max_connections_per_host);

        if let Some(all_proxy) = config.all_proxy {
            tracing::debug!(proxy = %all_proxy, "setting global proxy");
            client = client.proxy(Proxy::all(all_proxy).unwrap());
        }
        if let Some(host_proxy) = config.host_proxy {
            tracing::debug!(
                host_count = host_proxy.len(),
                "setting host-specific proxies"
            );
            let proxy = Proxy::custom(move |url| {
                let host_str = url.host_str()?;
                for (host, proxy) in host_proxy.iter() {
                    if host.to_string() == host_str.to_string() {
                        let proxy_url = Url::parse(proxy);
                        if proxy_url.is_err() {
                            break;
                        }
                        let proxy_url = proxy_url.unwrap();
                        return Some(proxy_url);
                    }
                }

                return None::<Url>;
            });
            client = client.proxy(proxy);
        }

        let client = client.build().map_err(|e| {
            tracing::debug!(error = %e, "build HTTP client error");
            HttpClientError::Network(e.to_string())
        })?;

        Ok(Self {
            encryption_provider: config.encryption_provider,
            decryption_provider: config.decryption_provider,
            cookie_store,
            client,
            _session,
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
    fn system_time_to_utf8(system_time: Option<SystemTime>) -> Option<DateTime<FixedOffset>> {
        if system_time.is_none() {
            return None;
        }

        let utc: DateTime<Utc> = system_time.unwrap().into();
        Some(utc.with_timezone(&FixedOffset::east_opt(8 * 3600).unwrap()))
    }

    #[tracing::instrument(skip(self, request_builder), parent = &self._session)]
    async fn inject_cookies(
        &self,
        url: &str,
        request_builder: reqwest::RequestBuilder,
    ) -> Result<reqwest::RequestBuilder, HttpClientError> {
        let cookies = self.cookie_store.get_for_url(url).await?;
        if cookies.is_empty() {
            tracing::debug!("cookies is empty");
            return Ok(request_builder);
        }

        tracing::debug!(cookie_count = %cookies.len(), "preparing cookie headers");
        let cookie_header: String = cookies
            .iter()
            .map(|(key, value)| format!("{}={}", key.name, value.value))
            .collect::<Vec<_>>()
            .join("; ");

        Ok(request_builder.header(
            reqwest::header::COOKIE,
            reqwest::header::HeaderValue::from_str(&cookie_header).map_err(|e| {
                tracing::error!(error = %e, "build cookie header value error");
                HttpClientError::InvalidHeader(e.to_string())
            })?,
        ))
    }

    #[tracing::instrument(skip(self, response), parent = &self._session)]
    async fn extract_cookies(&self, response: &Response) -> Result<(), HttpClientError> {
        tracing::debug!(url = %response.url(), "extracting cookies");

        if let Some(url) = response.url().host_str() {
            for cookie in response.cookies() {
                let name = cookie.name();
                let value = cookie.value();

                let first_same_site = match cookie.same_site_lax() {
                    true => SameSite::Lax,
                    false => SameSite::Strict,
                };
                let second_same_site = match cookie.same_site_strict() {
                    true => SameSite::Strict,
                    false => SameSite::Lax,
                };

                let same_site = if first_same_site != second_same_site {
                    None
                } else {
                    Some(first_same_site)
                };

                let now = Self::system_time_to_utf8(Some(SystemTime::now())).unwrap();
                let key = CookieKeysActiveModel {
                    id: ActiveValue::NotSet,
                    path: ActiveValue::Set(response.url().path().to_string()),
                    name: ActiveValue::Set(name.to_string()),
                    domain: ActiveValue::Set(url.to_string()),
                };
                let value = CookiesActiveModel {
                    id: ActiveValue::NotSet,
                    key_id: ActiveValue::NotSet,
                    value: ActiveValue::Set(value.to_string()),
                    expires_at: ActiveValue::Set(Self::system_time_to_utf8(cookie.expires())),
                    created_at: ActiveValue::Set(now.clone()),
                    last_access_at: ActiveValue::Set(now),
                    secure: ActiveValue::Set(cookie.secure()),
                    http_only: ActiveValue::Set(cookie.http_only()),
                    same_site: ActiveValue::Set(same_site),
                };

                self.cookie_store
                    .set(key, value, cookie.expires().is_some())
                    .await?;
            }
        }

        Ok(())
    }

    #[tracing::instrument(skip(self, endpoint), parent = &self._session)]
    async fn do_execute(&self, endpoint: HttpEndpoint) -> Result<Response, HttpClientError> {
        tracing::debug!(
            domain = ?endpoint.domain,
            path = ?endpoint.path,
            requires_encryption = ?endpoint.requires_encryption,
            requires_decryption = ?endpoint.requires_decryption,
            "do execute HTTP"
        );

        if endpoint.body.is_some()
            && endpoint.requires_encryption
            && self.encryption_provider.is_none()
        {
            tracing::error!("endpoint requires encryption but no encryption provider exists");
            return Err(HttpClientError::Configuration(
                "no encryption provider".to_string(),
            ));
        }
        if endpoint.body.is_some()
            && endpoint.requires_decryption
            && self.decryption_provider.is_none()
        {
            tracing::error!("endpoint requires decryption but no decryption provider exists");
            return Err(HttpClientError::Configuration(
                "no decryption provider".to_string(),
            ));
        }

        let method = Self::convert_method(&endpoint.method);
        let url = endpoint.build_url();
        let mut request_builder = self.client.request(method, &url);

        if endpoint.headers.is_some() {
            let headers = endpoint.headers.unwrap();
            tracing::debug!(header_count = ?headers.len(), "configuring headers");
            for (key, value) in headers {
                request_builder = request_builder.header(&key, value);
            }
        }

        if endpoint.user_agent.is_some() {
            let user_agent = endpoint.user_agent.unwrap();
            tracing::debug!(user_agent = ?user_agent, "configuring user_agent");
            request_builder = request_builder.header(reqwest::header::USER_AGENT, user_agent);
        }

        if endpoint.content_type.is_some() {
            let content_type = endpoint.content_type.unwrap();
            tracing::debug!(content_type = ?content_type, "configuring content_type");
            request_builder = request_builder.header(reqwest::header::CONTENT_TYPE, content_type);
        }

        if endpoint.body.is_some() {
            let body = endpoint.body.unwrap();
            let body = if endpoint.requires_encryption {
                let body = self.encryption_provider.as_ref().unwrap().encrypt(&body)?;
                body
            } else {
                body
            };
            request_builder = request_builder.body(body);
        }

        request_builder = self.inject_cookies(&url, request_builder).await?;

        let request = request_builder
            .timeout(endpoint.timeout)
            .build()
            .map_err(|e| HttpClientError::Configuration(e.to_string()))?;
        let response = self.client.execute(request).await.map_err(|e| {
            if e.is_timeout() {
                tracing::error!(error = %e, "execute timeout");
                HttpClientError::Timeout(endpoint.timeout)
            } else {
                tracing::error!(error = %e, "execute network error");
                HttpClientError::Network(e.to_string())
            }
        })?;

        self.extract_cookies(&response).await?;

        Ok(response)
    }
}

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

    #[tracing::instrument(skip(self, endpoint), parent = &self._session)]
    async fn execute(&self, endpoint: HttpEndpoint) -> Result<HttpResponse, HttpClientError> {
        tracing::debug!(
            domain = ?endpoint.domain,
            path = ?endpoint.path,
            requires_encryption = ?endpoint.requires_encryption,
            requires_decryption = ?endpoint.requires_decryption,
            "executing HTTP"
        );

        let requires_decryption = endpoint.requires_decryption;

        let response = self.do_execute(endpoint).await.inspect_err(|e| {
            tracing::error!(error = %e, "execute http error");
        })?;
        let status = response.status().as_u16();
        let headers: Vec<(String, String)> = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();

        let mut body: Vec<u8>;
        let content_length = response.content_length();
        if content_length.is_some() {
            tracing::info!(content_length = ?content_length, "reading response stream");

            let stream = response.bytes_stream();
            let stream = stream
                .map_err(|e| std::io::Error::new(ErrorKind::Other, e.to_string()))
                .inspect_err(|e| {
                    tracing::error!(error = %e, "read response stream error");
                });
            let async_read = stream.into_async_read();
            let tokio_async_read = async_read.compat();

            let mut reader = AsyncProgressReader::new(
                tokio_async_read,
                content_length.unwrap(),
                move |_, _, _| {},
            );
            body = Vec::new();

            tokio::io::copy(&mut reader, &mut body)
                .await
                .map_err(|e| HttpClientError::Network(e.to_string()))
                .inspect_err(|e| {
                    tracing::error!(error = %e, "copy response stream error");
                })?;
        } else {
            body = response
                .bytes()
                .await
                .map_err(|e| HttpClientError::Network(e.to_string()))
                .inspect_err(|e| {
                    tracing::error!(error = %e, "read response error");
                })?
                .to_vec();
        }

        if requires_decryption {
            body = self.decryption_provider.as_ref().unwrap().decrypt(&body)?;
        }

        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }

    #[tracing::instrument(skip(self, endpoint), parent = &self._session)]
    async fn execute_stream(
        &self,
        endpoint: HttpEndpoint,
    ) -> Result<HttpStreamResponse, HttpClientError> {
        tracing::debug!(
            domain = ?endpoint.domain,
            path = ?endpoint.path,
            requires_encryption = ?endpoint.requires_encryption,
            requires_decryption = ?endpoint.requires_decryption,
            "executing HTTP"
        );

        let response = self.do_execute(endpoint).await.inspect_err(|e| {
            tracing::error!(error = %e, "execute http error");
        })?;
        let status = response.status().as_u16();
        let headers: Vec<(String, String)> = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();
        let content_length = response.content_length();

        let stream = response
            .bytes_stream()
            .map_err(|e| HttpClientError::Network(e.to_string()))
            .inspect_err(|e| {
                tracing::error!(error = %e, "read response stream error");
            })
            .on_complete(move || {});

        if content_length.is_some() {
            tracing::debug!(content_length = ?content_length, "preparing response stream");
            let stream = Box::pin(stream);
            return Ok(HttpStreamResponse {
                status,
                headers,
                stream,
            });
        }

        let stream = Box::pin(stream);
        Ok(HttpStreamResponse {
            status,
            headers,
            stream,
        })
    }
}
