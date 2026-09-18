//! REPL client module

//! REPL client for F1r3fly node
//!
//! This module provides a gRPC client for interacting with the REPL service.

pub mod repl {
    tonic::include_proto!("repl");
}

use std::path::Path;
use std::time::Duration;

use futures::future::join_all;
use repl::repl_client::ReplClient;
use repl::{CmdRequest, ReplResponse};
use tokio::fs;
use tonic::transport::{Channel, Endpoint};
use tonic::Status;

use crate::rust::effects::repl_client::repl::EvalRequest;

pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) fn validate_request_timeout(timeout: Duration) -> Result<Duration, &'static str> {
    if timeout.is_zero() {
        return Err("timeout must be positive");
    }
    if std::time::Instant::now().checked_add(timeout).is_none() {
        return Err("timeout exceeds the platform clock range");
    }
    Ok(timeout)
}

/// Trait for REPL client operations
#[async_trait::async_trait]
pub trait ReplClientService {
    /// Run a single line of code
    async fn run(&self, line: String) -> eyre::Result<String>;

    /// Evaluate multiple files
    async fn eval_files(
        &self,
        file_names: &Vec<String>,
        print_unmatched_sends_only: bool,
        language: String,
    ) -> Vec<eyre::Result<String>>;

    /// Evaluate a single file
    async fn eval_file(
        &self,
        file_name: String,
        print_unmatched_sends_only: bool,
        language: String,
    ) -> eyre::Result<String>;
}

/// gRPC REPL client implementation
pub struct GrpcReplClient {
    client: ReplClient<Channel>,
}

impl GrpcReplClient {
    /// Create a new gRPC REPL client
    pub async fn new(
        host: String,
        port: u16,
        max_message_size: usize,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Self::with_timeout(host, port, max_message_size, DEFAULT_REQUEST_TIMEOUT).await
    }

    pub async fn with_timeout(
        host: String,
        port: u16,
        max_message_size: usize,
        timeout: Duration,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let timeout = validate_request_timeout(timeout)
            .map_err(|message| std::io::Error::new(std::io::ErrorKind::InvalidInput, message))?;
        let endpoint = Endpoint::from_shared(format!("http://{host}:{port}"))?
            .connect_timeout(Duration::from_secs(5)) // TODO adjust the connect_timeout if necessary
            .timeout(timeout);

        let channel = endpoint.connect().await?;

        Ok(Self {
            client: ReplClient::new(channel).max_decoding_message_size(max_message_size),
        })
    }

    /// Read content from a file
    async fn read_content(file_path: &Path) -> eyre::Result<String> {
        let content = fs::read_to_string(file_path).await?;
        Ok(content)
    }

    /// Process gRPC errors
    fn process_error(error: Status) -> eyre::Report {
        // Extract the root cause if available
        let message = error.message().to_string();
        eyre::Report::new(std::io::Error::other(message))
    }
}

#[cfg(test)]
mod eval_timeout_tests {
    use super::*;
    use crate::rust::api::repl_grpc_service::repl::repl_server::{Repl, ReplServer};
    use crate::rust::api::repl_grpc_service::repl::{
        CmdRequest as ServerCmdRequest, EvalRequest as ServerEvalRequest,
        ReplResponse as ServerReplResponse,
    };

    struct DelayedRepl;

    #[tonic::async_trait]
    impl Repl for DelayedRepl {
        async fn run(
            &self,
            _: tonic::Request<ServerCmdRequest>,
        ) -> Result<tonic::Response<ServerReplResponse>, Status> {
            Err(Status::unimplemented("eval-only test service"))
        }

        async fn eval(
            &self,
            request: tonic::Request<ServerEvalRequest>,
        ) -> Result<tonic::Response<ServerReplResponse>, Status> {
            let source = request.into_inner().program;
            if source == "wait" {
                std::future::pending::<()>().await;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
            Ok(tonic::Response::new(ServerReplResponse { output: source }))
        }
    }

    #[test]
    fn default_is_thirty_seconds_and_invalid_deadlines_refuse() {
        assert_eq!(DEFAULT_REQUEST_TIMEOUT, Duration::from_secs(30));
        assert!(validate_request_timeout(Duration::ZERO).is_err());
        assert!(validate_request_timeout(Duration::MAX).is_err());
        assert_eq!(
            validate_request_timeout(Duration::from_secs(300)),
            Ok(Duration::from_secs(300))
        );
    }

    #[tokio::test]
    async fn invalid_timeout_refuses_before_connection() {
        for duration in [Duration::ZERO, Duration::MAX] {
            let error = GrpcReplClient::with_timeout("127.0.0.1".into(), 0, 4096, duration)
                .await
                .err()
                .expect("invalid duration must refuse");
            assert!(error.to_string().contains("timeout"));
        }
    }

    #[tokio::test]
    async fn configured_eval_timeout_expires_and_longer_wait_returns_response() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind isolated test listener");
        let port = listener.local_addr().expect("listener address").port();
        let incoming = futures::stream::unfold(listener, |listener| async move {
            let accepted = listener.accept().await.map(|(stream, _)| stream);
            Some((accepted, listener))
        });
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(ReplServer::new(DelayedRepl))
                .serve_with_incoming_shutdown(incoming, async {
                    let _ = stopped.await;
                })
                .await
        });
        let request = |source: &str| EvalRequest {
            program: source.into(),
            language: "rho".into(),
            print_unmatched_sends_only: false,
        };
        let mut short =
            GrpcReplClient::with_timeout("127.0.0.1".into(), port, 4096, Duration::from_millis(30))
                .await
                .expect("connect short-deadline client");
        let refused =
            tokio::time::timeout(Duration::from_secs(5), short.client.eval(request("wait")))
                .await
                .expect("client deadline must terminate pending RPC")
                .expect_err("pending RPC must time out");
        assert!(refused.message().contains("Timeout expired"), "{refused}");
        drop(short);
        let mut longer =
            GrpcReplClient::with_timeout("127.0.0.1".into(), port, 4096, Duration::from_secs(5))
                .await
                .expect("connect longer-deadline client");
        let response = longer
            .client
            .eval(request("unaltered response"))
            .await
            .expect("longer deadline admits delayed response")
            .into_inner();
        assert_eq!(response.output, "unaltered response");
        drop(longer);
        stop.send(()).expect("stop isolated test server");
        tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .expect("server shuts down")
            .expect("server task joins")
            .expect("server succeeds");
    }
}

#[async_trait::async_trait]
impl ReplClientService for GrpcReplClient {
    async fn run(&self, line: String) -> eyre::Result<String> {
        let req = CmdRequest { line: line.into() };

        // Call the RPC
        match self.client.clone().run(req).await {
            Ok(resp) => {
                let ReplResponse { output } = resp.into_inner();
                Ok(output)
            }
            Err(status) => Err(Self::process_error(status)),
        }
    }

    async fn eval_files(
        &self,
        file_names: &Vec<String>,
        print_unmatched_sends_only: bool,
        language: String,
    ) -> Vec<eyre::Result<String>> {
        join_all(file_names.iter().map(|file_name| async {
            self.eval_file(
                file_name.clone(),
                print_unmatched_sends_only,
                language.clone(),
            )
            .await
        }))
        .await
    }

    async fn eval_file(
        &self,
        file_name: String,
        print_unmatched_sends_only: bool,
        language: String,
    ) -> eyre::Result<String> {
        let file_path = Path::new(&file_name);

        if !file_path.exists() {
            return Err(eyre::Report::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "File not found",
            )));
        }

        let content = Self::read_content(file_path).await?;

        let req = EvalRequest {
            program: content,
            print_unmatched_sends_only,
            language,
        };

        // Call the RPC
        match self.client.clone().eval(req).await {
            Ok(resp) => {
                let ReplResponse { output } = resp.into_inner();
                Ok(output)
            }
            Err(status) => Err(Self::process_error(status)),
        }
    }
}
