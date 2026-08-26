use async_trait::async_trait;
use bytes::Bytes;
use eyre::Result;
use opentelemetry_http::{HttpClient, HttpError};
use shared::rust::tracing_init::BoxedTracingLayer;

#[derive(Debug, Clone)]
struct ReqwestHttpClient {
    client: reqwest::Client,
}

#[async_trait]
impl HttpClient for ReqwestHttpClient {
    async fn send(
        &self,
        request: http_02::Request<Vec<u8>>,
    ) -> Result<http_02::Response<Bytes>, HttpError> {
        let (parts, body) = request.into_parts();
        let method = reqwest::Method::from_bytes(parts.method.as_str().as_bytes())?;
        let mut outgoing = self.client.request(method, parts.uri.to_string());
        for (name, value) in &parts.headers {
            outgoing = outgoing.header(name.as_str(), value.as_bytes());
        }
        let response = outgoing.body(body).send().await?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = response.bytes().await?;
        let mut incoming = http_02::Response::builder().status(status).body(body)?;
        for (name, value) in &headers {
            incoming.headers_mut().insert(
                http_02::header::HeaderName::from_bytes(name.as_str().as_bytes())?,
                http_02::header::HeaderValue::from_bytes(value.as_bytes())?,
            );
        }
        Ok(incoming)
    }
}

pub struct ZipkinGuard;

impl Drop for ZipkinGuard {
    fn drop(&mut self) { opentelemetry::global::shutdown_tracer_provider() }
}

pub fn create_zipkin_reporter() -> Result<(BoxedTracingLayer, ZipkinGuard)> {
    create_zipkin_reporter_with_endpoint(None)
}

fn create_zipkin_reporter_with_endpoint(
    collector_endpoint: Option<String>,
) -> Result<(BoxedTracingLayer, ZipkinGuard)> {
    let client = ReqwestHttpClient {
        client: reqwest::Client::builder().build()?,
    };
    opentelemetry::global::set_text_map_propagator(opentelemetry_zipkin::Propagator::new());
    let mut pipeline = opentelemetry_zipkin::new_pipeline()
        .with_http_client(client)
        .with_service_name("f1r3node");
    if let Some(endpoint) = collector_endpoint {
        pipeline = pipeline.with_collector_endpoint(endpoint);
    }
    let tracer = pipeline.install_batch(opentelemetry::runtime::Tokio)?;
    let layer = tracing_opentelemetry::layer().with_tracer(tracer);
    Ok((Box::new(layer), ZipkinGuard))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::time::timeout;
    use tracing_subscriber::layer::SubscriberExt;

    use super::create_zipkin_reporter_with_endpoint;

    async fn capture_request(listener: TcpListener) -> Vec<u8> {
        let (mut stream, _) = listener.accept().await.expect("collector connection");
        let mut request = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            let read = stream.read(&mut buffer).await.expect("collector read");
            assert!(
                read > 0,
                "collector connection closed before request completed"
            );
            request.extend_from_slice(&buffer[..read]);
            let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n") else {
                continue;
            };
            let header_end = header_end + 4;
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().expect("content length"))
                })
                .expect("content-length header");
            if request.len() >= header_end + content_length {
                break;
            }
        }
        stream
            .write_all(b"HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .expect("collector response");
        request
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reporter_exports_zipkin_v2_span() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("collector bind");
        let endpoint = format!("http://{}/api/v2/spans", listener.local_addr().unwrap());
        let request = tokio::spawn(capture_request(listener));
        let (layer, guard) =
            create_zipkin_reporter_with_endpoint(Some(endpoint)).expect("create Zipkin reporter");
        let subscriber = tracing_subscriber::registry().with(layer);

        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("zipkin_regression", account = "vault");
            let _entered = span.enter();
            tracing::info!(charge = 7u64, "settled");
        });
        drop(guard);

        let request = timeout(Duration::from_secs(10), request)
            .await
            .expect("Zipkin export timeout")
            .expect("collector task");
        let header_end = request
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .expect("HTTP headers")
            + 4;
        let headers = String::from_utf8_lossy(&request[..header_end]);
        assert!(headers.starts_with("POST /api/v2/spans HTTP/1.1"));
        assert!(headers
            .lines()
            .any(|line| line.eq_ignore_ascii_case("content-type: application/json")));
        let spans: serde_json::Value =
            serde_json::from_slice(&request[header_end..]).expect("Zipkin JSON");
        let spans = spans.as_array().expect("Zipkin span array");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0]["name"], "zipkin_regression");
        assert_eq!(spans[0]["localEndpoint"]["serviceName"], "f1r3node");
    }
}
