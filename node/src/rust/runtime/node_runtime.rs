// See node/src/main/scala/coop/rchain/node/runtime/NodeRuntime.scala

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use comm::rust::peer_node::NodeIdentifier;
use tokio::task::JoinSet;
use tracing::info;

use crate::rust::configuration::NodeConf;
use crate::rust::effects::node_discover;
use crate::rust::node_environment;

/// Wrapper for task results that includes the task name for identification
#[derive(Debug)]
struct NamedTaskResult {
    name: String,
    result: eyre::Result<()>,
}

/// Spawn a named task in a JoinSet
///
/// Wraps the task result with its name so we can identify which task completed.
fn spawn_named_task(
    join_set: &mut JoinSet<NamedTaskResult>,
    name: impl Into<String>,
    task: impl Future<Output = eyre::Result<()>> + Send + 'static,
) {
    let name = name.into();
    join_set.spawn(async move {
        let result = task.await;
        NamedTaskResult { name, result }
    });
}

/// NodeRuntime - Main entry point for running an F1r3node
///
/// This struct manages the entire lifecycle of a node, including:
/// - Initialization of all subsystems
/// - Orchestration of concurrent tasks (Casper, block processing, networking)
/// - Server management (gRPC, HTTP, Kademlia)
/// - Graceful shutdown and cleanup
pub struct NodeRuntime {
    node_conf: NodeConf,
    id: NodeIdentifier,
}

impl NodeRuntime {
    /// Create a new NodeRuntime instance
    ///
    /// # Arguments
    /// * `node_conf` - Node configuration
    /// * `id` - Node identifier derived from TLS certificate
    pub fn new(node_conf: NodeConf, id: NodeIdentifier) -> Self { Self { node_conf, id } }

    /// Main node entry point
    ///
    /// Initializes all node subsystems and starts the main runtime loop.
    ///
    /// Returns `Ok(())` on successful node shutdown, or an error if initialization fails
    pub async fn main(&self) -> eyre::Result<()> {
        info!("NodeRuntime.main() called");

        // Fetch local peer node
        let local = comm::rust::who_am_i::fetch_local_peer_node(
            self.node_conf.protocol_server.host.clone(),
            self.node_conf.protocol_server.port,
            self.node_conf.peers_discovery.port,
            self.node_conf.protocol_server.no_upnp,
            self.id.clone(),
        )
        .await
        .map_err(|e| eyre::eyre!("Failed to fetch local peer node: {}", e))?;

        info!("Local peer node: {}", local.to_address());

        // Metrics and time effects stubbed for now (coming in separate PR)
        let _metrics = (); // Placeholder
        let _time = (); // Placeholder

        // Create transport client
        let transport = {
            use std::collections::HashMap;

            use comm::rust::transport::grpc_transport_client::GrpcTransportClient;

            let channels_map = Arc::new(tokio::sync::Mutex::new(HashMap::new()));

            // Read certificate and key file contents
            let cert = tokio::fs::read_to_string(&self.node_conf.tls.certificate_path)
                .await
                .map_err(|e| eyre::eyre!("Failed to read certificate file: {}", e))?;
            let key = tokio::fs::read_to_string(&self.node_conf.tls.key_path)
                .await
                .map_err(|e| eyre::eyre!("Failed to read key file: {}", e))?;

            const CLIENT_QUEUE_SIZE: i32 = 100;

            GrpcTransportClient::new(
                self.node_conf.protocol_client.network_id.clone(),
                cert,
                key,
                self.node_conf.protocol_client.grpc_max_recv_message_size as i32,
                self.node_conf.protocol_client.grpc_stream_chunk_size as i32,
                CLIENT_QUEUE_SIZE,
                channels_map,
                self.node_conf.protocol_client.network_timeout,
            )
            .map_err(|e| eyre::eyre!("Failed to create transport client: {}", e))?
        };

        info!("Transport client created successfully");

        // Create RP connections
        let rp_connections = comm::rust::rp::connect::ConnectionsCell::new();

        let init_peer =
            if self.node_conf.standalone || self.node_conf.protocol_client.bootstrap.is_empty() {
                None
            } else {
                Some(
                    comm::rust::peer_node::PeerNode::from_address(
                        &self.node_conf.protocol_client.bootstrap,
                    )
                    .map_err(|e| eyre::eyre!("Failed to parse bootstrap peer address: {}", e))?,
                )
            };

        // Create RPConf
        let rp_conf = comm::rust::rp::rp_conf::RPConf::new(
            local.clone(),
            self.node_conf.protocol_client.network_id.clone(),
            init_peer.clone(),
            self.node_conf.protocol_client.network_timeout,
            self.node_conf.protocol_client.batch_max_connections as usize,
            self.node_conf.peers_discovery.heartbeat_batch_size as usize,
        );

        // Wrap RPConf in RPConfCell for shared mutable access (allows dynamic IP updates)
        let rp_conf_cell = comm::rust::rp::rp_conf::RPConfCell::new(rp_conf.clone());

        // Create KademliaRPC
        let kademlia_rpc = {
            use comm::rust::discovery::grpc_kademlia_rpc::GrpcKademliaRPC;

            Arc::new(GrpcKademliaRPC::new(
                self.node_conf.protocol_server.network_id.clone(),
                self.node_conf.protocol_client.network_timeout,
                self.node_conf.protocol_server.allow_private_addresses,
                local.clone(),
            ))
        };

        info!("KademliaRPC initialized");

        // Create KademliaStore
        let kademlia_store = {
            use comm::rust::discovery::kademlia_store::KademliaStore;

            Arc::new(KademliaStore::new(self.id.clone(), kademlia_rpc.clone()))
        };

        info!("KademliaStore initialized");

        // Update bootstrap peer's last seen timestamp
        if let Some(ref peer) = init_peer {
            kademlia_store
                .update_last_seen(peer)
                .await
                .map_err(|e| eyre::eyre!("Failed to update last seen for bootstrap peer: {}", e))?;
            info!(
                "Updated last seen timestamp for bootstrap peer: {}",
                peer.to_address()
            );
        }

        // Create NodeDiscovery
        let node_discovery = node_discover(
            self.id.clone(),
            kademlia_rpc.clone(),
            kademlia_store.clone(),
        )
        .await?;

        info!("NodeDiscovery initialized");

        // Create event bus backed by a broadcast channel (capacity: 100).
        // Events published during startup are buffered for replay to
        // late-connecting WebSocket clients. The buffer is sealed when
        // engine_init completes (see the tokio::select! loop below).
        let event_bus = {
            use shared::rust::shared::f1r3fly_events::F1r3flyEvents;

            F1r3flyEvents::new()
        };

        info!("Event bus (F1r3flyEvents) initialized");

        // Call setup_node_program to initialize core components
        info!("Calling setup_node_program...");

        let result = crate::rust::runtime::setup::setup_node_program(
            rp_connections.clone(),
            rp_conf_cell.clone(),
            Arc::new(transport.clone()),
            self.node_conf.clone(),
            event_bus.clone(),
            node_discovery.clone(),
        )
        .await?;

        let crate::rust::runtime::setup::PreparedNode {
            consensus,
            packet_handler,
            application,
        } = result;
        let program = self.node_program(
            application,
            consensus,
            transport,
            rp_conf_cell,
            rp_connections,
            kademlia_store,
            node_discovery,
            packet_handler,
            event_bus,
        );

        // Wrap with error handling
        handle_unrecoverable_errors(program).await
    }

    /// Node program - orchestrates all concurrent tasks
    ///
    /// Coordinates all long-running tasks that make up the node:
    /// - API servers (gRPC and HTTP)
    /// - Casper consensus loops
    /// - Block processing
    /// - Proposer (if validator)
    /// - Network discovery and connection management
    #[allow(clippy::too_many_arguments)]
    async fn node_program(
        &self,
        application: crate::rust::runtime::setup::PreparedApplication,
        consensus: consensus_runtime::PreparedConsensus,
        transport: comm::rust::transport::grpc_transport_client::GrpcTransportClient,
        rp_conf_cell: comm::rust::rp::rp_conf::RPConfCell,
        rp_connections: comm::rust::rp::connect::ConnectionsCell,
        kademlia_store: Arc<
            comm::rust::discovery::kademlia_store::KademliaStore<
                comm::rust::discovery::grpc_kademlia_rpc::GrpcKademliaRPC,
            >,
        >,
        node_discovery: Arc<dyn comm::rust::discovery::node_discovery::NodeDiscovery + Send + Sync>,
        packet_handler: Arc<
            dyn comm::rust::p2p::packet_handler::PacketHandler + Send + Sync + 'static,
        >,
        event_bus: shared::rust::shared::f1r3fly_events::F1r3flyEvents,
    ) -> eyre::Result<()> {
        // Display node startup info
        if self.node_conf.standalone {
            info!("Starting stand-alone node.");
        } else if self.node_conf.protocol_client.bootstrap.is_empty() {
            info!("Starting node with no bootstrap peer.");
        } else {
            info!(
                "Starting node that will bootstrap from {}",
                self.node_conf.protocol_client.bootstrap
            );
        }

        // Get local peer configuration
        let rp_conf = rp_conf_cell
            .read()
            .map_err(|e| eyre::eyre!("Failed to read RPConf: {}", e))?;
        let local = rp_conf.local.clone();

        let address = local.to_address();
        let host = local.endpoint.host.clone();

        info!("Node address: {}, host: {}", address, host);

        // Build server instances
        info!("Building server instances...");

        let event_stream = event_bus.consume();

        // Create gRPC packet handler closure
        let grpc_packet_handler = {
            let transport_arc = Arc::new(transport.clone());
            let packet_handler_arc = packet_handler.clone();
            let rp_connections_clone = rp_connections.clone();
            let rp_conf_clone = rp_conf.clone();

            Arc::new(move |protocol: models::routing::Protocol| {
                let transport = transport_arc.clone();
                let handler = packet_handler_arc.clone();
                let connections = rp_connections_clone.clone();
                let conf = rp_conf_clone.clone();

                Box::pin(async move {
                    use comm::rust::rp::handle_messages;
                    handle_messages::handle(&protocol, transport, handler, &connections, &conf).await
                })
                    as Pin<
                        Box<
                            dyn Future<
                                    Output = Result<
                                        comm::rust::transport::communication_response::CommunicationResponse,
                                        comm::rust::errors::CommError,
                                    >,
                                > + Send,
                        >,
                    >
            })
        };

        // Create gRPC blob handler closure
        let grpc_blob_handler = {
            let packet_handler_arc = packet_handler.clone();

            Arc::new(move |blob: comm::rust::transport::transport_layer::Blob| {
                let handler = packet_handler_arc.clone();

                Box::pin(async move { handler.handle_packet(&blob.sender, &blob.packet).await })
                    as Pin<
                        Box<dyn Future<Output = Result<(), comm::rust::errors::CommError>> + Send>,
                    >
            })
        };

        // Build all server instances
        let mut servers = crate::rust::runtime::servers_instances::ServersInstances::build(
            application,
            grpc_packet_handler,
            grpc_blob_handler,
            &host,
            &address,
            self.node_conf.clone(),
            rp_conf_cell.clone(),
            rp_connections.clone(),
            node_discovery.clone(),
            event_stream,
            event_bus.startup_buffer(),
            kademlia_store.clone(),
        )
        .await?;

        info!("All servers started successfully");

        // Publish NodeStarted event
        let node_started_event =
            shared::rust::shared::f1r3fly_event::F1r3flyEvent::node_started(address.clone());
        event_bus
            .publish(node_started_event)
            .map_err(|e| eyre::eyre!("Failed to publish NodeStarted event: {}", e))?;

        info!("NodeStarted event published: {}", address);

        // Start all concurrent tasks with categorized failure handling
        // Critical tasks: Failure triggers immediate shutdown
        // Supportive tasks: Failure is logged as warning, node continues
        let mut critical_tasks: JoinSet<NamedTaskResult> = JoinSet::new();

        // === CRITICAL TASKS: Tier 1 - Server Infrastructure ===
        // All server failures are critical - if a server dies, the node cannot function properly

        info!("Starting critical server tasks...");

        // Transport server
        spawn_named_task(
            &mut critical_tasks,
            "Transport Server",
            await_http_server_task(servers.transport_server_handle, "Transport"),
        );

        // Kademlia server
        spawn_named_task(
            &mut critical_tasks,
            "Kademlia Server",
            await_server_task(servers.kademlia_server_handle, "Kademlia"),
        );

        // Node discovery loop (Tier 3: Supportive)
        // Start BEFORE wait_for_first_connection so it can help establish the first connection
        let nd_clone = node_discovery.clone();
        let rp_conn_clone = rp_connections.clone();
        let rp_conf_cell_clone = rp_conf_cell.clone();
        let transport_clone = transport.clone();
        let node_conf_clone = self.node_conf.clone();

        spawn_named_task(&mut critical_tasks, "Node Discovery Loop", async move {
            node_discovery_loop(
                nd_clone,
                rp_conn_clone,
                rp_conf_cell_clone,
                transport_clone,
                node_conf_clone,
            )
            .await
        });

        // Clear connections loop (Tier 3: Supportive)
        // Start BEFORE wait_for_first_connection to match Scala connectivityStream behavior
        let rp_conn_clone2 = rp_connections.clone();
        let rp_conf_cell_clone2 = rp_conf_cell.clone();
        let transport_clone2 = transport.clone();
        let nd_clone2 = node_discovery.clone();
        let node_conf_clone2 = self.node_conf.clone();

        spawn_named_task(&mut critical_tasks, "Clear Connections Loop", async move {
            clear_connections_loop(
                rp_conn_clone2,
                rp_conf_cell_clone2,
                transport_clone2,
                nd_clone2,
                node_conf_clone2,
            )
            .await
        });

        let mut consensus = consensus.start();

        // === CRITICAL TASKS: Tier 3 - API Servers ===
        // These are critical for the node to function properly

        info!("Starting API server tasks...");

        // External API server
        spawn_named_task(
            &mut critical_tasks,
            "External API Server",
            await_server_task(servers.external_api_server_handle, "External API"),
        );

        // Internal API server
        spawn_named_task(
            &mut critical_tasks,
            "Internal API Server",
            await_server_task(servers.internal_api_server_handle, "Internal API"),
        );

        // HTTP server
        spawn_named_task(
            &mut critical_tasks,
            "HTTP Server",
            await_http_server_task(servers.http_server_handle, "HTTP"),
        );

        // Admin HTTP server
        spawn_named_task(
            &mut critical_tasks,
            "Admin HTTP Server",
            await_http_server_task(servers.admin_http_server_handle, "Admin HTTP"),
        );

        info!("All server tasks started successfully");

        // === Monitor both JoinSets for failures ===
        // Critical tasks: Any failure triggers immediate shutdown
        // Supportive tasks: Failures are logged as warnings, node continues
        info!("All tasks started. Node is now running.");

        let result = loop {
            tokio::select! {
                Some(result) = critical_tasks.join_next() => {
                    break match result {
                        Ok(task) => task.result.and_then(|_| Err(eyre::eyre!("Task {} completed unexpectedly", task.name))),
                        Err(error) => Err(error.into()),
                    };
                }
                result = consensus.wait() => {
                    break result.map_err(eyre::Report::new).and_then(|_| Err(eyre::eyre!("Consensus stopped unexpectedly")));
                }
                _ = shutdown_signal() => break Ok(()),
            }
        };
        let consensus_shutdown = consensus.shutdown().await;
        servers.shutdown.signal();
        let _ = servers.kademlia_server.stop().await;
        let _ = servers.external_api_server.stop().await;
        let _ = servers.internal_api_server.stop().await;
        let _ = servers.transport_server.stop().await;

        // === SHUTDOWN SEQUENCE ===
        info!("Shutting down all tasks...");

        // Step 1: Abort all tasks with 30-second timeout
        match tokio::time::timeout(std::time::Duration::from_secs(30), async {
            critical_tasks.shutdown().await;
        })
        .await
        {
            Ok(_) => info!("All tasks shut down gracefully"),
            Err(_) => {
                tracing::warn!("Shutdown timeout reached after 30 seconds, forcing termination");
            }
        }

        // Step 2: Perform resource cleanup (metrics, logging, etc.)
        // Block store cleanup happens automatically via Drop when block_store goes out of scope
        match Self::perform_shutdown_cleanup().await {
            Ok(_) => info!("Resource cleanup completed successfully"),
            Err(e) => {
                tracing::error!(error = %e, "resource cleanup during shutdown failed");
                // Continue anyway - we've already stopped the tasks
            }
        }

        info!("Node shutdown complete");
        result?;
        consensus_shutdown.map_err(eyre::Report::new)
    }

    /// Perform shutdown cleanup
    ///
    /// Cleanup operations:
    /// - Stops metrics reporters (stubbed - coming in separate PR)
    /// - Logs shutdown messages
    ///
    /// Note: Block store cleanup is handled automatically by Rust's Drop implementation
    /// when variables go out of scope. This matches Scala's approach which relies on
    /// Resource finalizers rather than explicit close() calls.
    async fn perform_shutdown_cleanup() -> eyre::Result<()> {
        info!("Starting shutdown cleanup...");

        // Stop metrics reporters (TODO: coming in separate PR)
        info!("Metrics cleanup skipped (coming in separate PR)");

        // Block store cleanup is handled automatically by Drop implementation
        // when the block_store variable goes out of scope
        info!("Bringing BlockStore down ...");
        info!("Block store will be closed automatically via Drop implementation");

        info!("Goodbye.");

        Ok(())
    }
}

/// Wait for shutdown signal (CTRL+C or SIGTERM)
///
/// Handles:
/// - CTRL+C (cross-platform)
/// - SIGTERM (Unix only)
///
/// Returns when either signal is received
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install CTRL+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received CTRL+C signal");
        },
        _ = terminate => {
            info!("Received SIGTERM signal");
        },
    }
}

/// Node discovery loop - runs indefinitely
///
/// Periodically discovers new peers and attempts to connect to them.
/// Uses configured lookup_interval from peers_discovery settings.
async fn node_discovery_loop(
    node_discovery: Arc<dyn comm::rust::discovery::node_discovery::NodeDiscovery + Send + Sync>,
    connections: comm::rust::rp::connect::ConnectionsCell,
    rp_conf_cell: comm::rust::rp::rp_conf::RPConfCell,
    transport: comm::rust::transport::grpc_transport_client::GrpcTransportClient,
    node_conf: NodeConf,
) -> eyre::Result<()> {
    use tokio::time::sleep;

    loop {
        tracing::debug!("nodeDiscoveryLoop: Starting iteration");

        // Discover new peers
        if let Err(e) = node_discovery.discover().await {
            tracing::warn!("Node discovery failed: {}", e);
        }

        // Read current RPConf (in case IP has changed)
        let rp_conf = match rp_conf_cell.read() {
            Ok(conf) => conf,
            Err(e) => {
                tracing::warn!("Failed to read RPConf: {}", e);
                sleep(node_conf.peers_discovery.lookup_interval).await;
                continue;
            }
        };

        // Find and connect to new peers using the proper find_and_connect function
        use comm::rust::rp::connect;

        // Create the connect closure
        let connect_fn = |peer: &comm::rust::peer_node::PeerNode| {
            let conf = rp_conf.clone();
            let transport = transport.clone();
            let peer = peer.clone();
            async move { connect::connect(&peer, &conf, &transport).await }
        };

        // Use find_and_connect with trait object
        match connect::find_and_connect(&connections, node_discovery.as_ref(), connect_fn).await {
            Ok(new_connections) => {
                if !new_connections.is_empty() {
                    info!("Connected to {} new peer(s)", new_connections.len());
                }
            }
            Err(e) => {
                tracing::warn!("Find and connect failed: {}", e);
            }
        }

        // Sleep for configured lookup interval before next iteration
        sleep(node_conf.peers_discovery.lookup_interval).await;
    }
}

/// Clear connections loop - runs indefinitely
///
/// Periodically checks connection health by sending heartbeats and removes
/// failed peers. Also handles dynamic IP changes and orphaned channel cleanup.
/// Uses configured cleanup_interval from peers_discovery settings.
///
/// The clear_connections function handles the full cleanup cycle:
/// 1. Removes peers from ConnectionsCell
/// 2. Removes peers from KademliaStore (except bootstrap, which is pinned)
/// 3. Disconnects the gRPC channel immediately (via transport.disconnect)
async fn clear_connections_loop(
    connections: comm::rust::rp::connect::ConnectionsCell,
    rp_conf_cell: comm::rust::rp::rp_conf::RPConfCell,
    transport: comm::rust::transport::grpc_transport_client::GrpcTransportClient,
    node_discovery: Arc<dyn comm::rust::discovery::node_discovery::NodeDiscovery + Send + Sync>,
    node_conf: NodeConf,
) -> eyre::Result<()> {
    use comm::rust::transport::transport_layer::TransportLayer;
    use tokio::time::sleep;

    let mut liveness = comm::rust::rp::connect::PeerLivenessTracker::new(
        node_conf.peers_discovery.heartbeat_failure_threshold,
    )?;

    loop {
        tracing::debug!("clearConnectionsLoop: Starting iteration");

        // Read current RPConf
        let rp_conf = match rp_conf_cell.read() {
            Ok(conf) => conf,
            Err(e) => {
                tracing::warn!("Failed to read RPConf: {}", e);
                sleep(node_conf.peers_discovery.cleanup_interval).await;
                continue;
            }
        };

        // Dynamic IP check - detect and handle external IP address changes
        if node_conf.protocol_server.dynamic_ip {
            match comm::rust::who_am_i::check_local_peer_node(
                node_conf.protocol_server.port,
                node_conf.peers_discovery.port,
                &rp_conf.local,
            )
            .await
            {
                Ok(Some(new_local)) => {
                    info!(
                        "External IP address has changed to {}, updating RPConf",
                        new_local.to_address()
                    );

                    // Update RPConf with new local peer
                    if let Err(e) = rp_conf_cell.update_local(new_local.clone()) {
                        tracing::error!(error = %e, "RPConf local peer update failed");
                    } else {
                        info!("Successfully updated RPConf with new local peer");

                        // Reset all connections since our address changed
                        if let Err(e) = comm::rust::rp::connect::reset_connections(&connections) {
                            tracing::warn!("Failed to reset connections: {}", e);
                        } else {
                            info!("Reset all connections after IP change");
                        }
                    }
                }
                Ok(None) => {
                    // IP hasn't changed, continue normally
                }
                Err(e) => {
                    tracing::warn!("Failed to check for IP change: {}", e);
                }
            }
        }

        // Re-read RPConf in case it was updated
        let rp_conf = match rp_conf_cell.read() {
            Ok(conf) => conf,
            Err(e) => {
                tracing::warn!("Failed to read RPConf after IP check: {}", e);
                sleep(node_conf.peers_discovery.cleanup_interval).await;
                continue;
            }
        };

        // Clear connections: heartbeats, ConnectionsCell update, Kademlia removal
        // (with bootstrap pinning), and gRPC disconnect — all handled inside clear_connections.
        match comm::rust::rp::connect::clear_connections(
            &connections,
            &rp_conf,
            &transport,
            &*node_discovery,
            &mut liveness,
        )
        .await
        {
            Ok((cleared_count, _failed_peers)) => {
                if cleared_count > 0 {
                    info!("Cleared {} failed connection(s)", cleared_count);
                }
            }
            Err(e) => {
                tracing::warn!("Clear connections failed: {}", e);
            }
        }

        // Clean up orphaned channels - channels for peers no longer in ConnectionsCell
        let updated_conns = match connections.read() {
            Ok(conns) => conns,
            Err(e) => {
                tracing::warn!("Failed to read connections for orphaned cleanup: {}", e);
                sleep(node_conf.peers_discovery.cleanup_interval).await;
                continue;
            }
        };

        match transport.get_channeled_peers().await {
            Ok(channeled_peers) => {
                let connection_set: std::collections::HashSet<_> =
                    updated_conns.iter().cloned().collect();
                let orphaned_peers: Vec<_> = channeled_peers
                    .iter()
                    .filter(|p| !connection_set.contains(*p))
                    .cloned()
                    .collect();

                if !orphaned_peers.is_empty() {
                    tracing::debug!("Disconnecting {} orphaned channels", orphaned_peers.len());
                    for peer in orphaned_peers {
                        tracing::debug!("Orphaned channel cleanup: {}", peer);
                        if let Err(e) = transport.disconnect(&peer).await {
                            tracing::warn!("Failed to disconnect orphaned peer {}: {}", peer, e);
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to get channeled peers: {}", e);
            }
        }

        // Sleep for configured cleanup interval before next iteration
        sleep(node_conf.peers_discovery.cleanup_interval).await;
    }
}

/// Helper function to await gRPC server task completion with proper error wrapping
///
/// Wraps a gRPC server `JoinHandle` to provide:
/// - Named server identification in logs
/// - Error type conversion (tonic::transport::Error -> eyre::Error)
/// - Graceful handling of task panics
///
/// # Arguments
/// * `handle` - The server's JoinHandle
/// * `server_name` - Human-readable name for logging
///
/// # Returns
/// Returns `Ok(())` if server completes successfully, or an error if it fails/panics
async fn await_server_task(
    handle: tokio::task::JoinHandle<Result<(), tonic::transport::Error>>,
    server_name: &str,
) -> eyre::Result<()> {
    match handle.await {
        Ok(Ok(())) => {
            tracing::warn!(
                "{} server completed unexpectedly (should run forever)",
                server_name
            );
            Ok(())
        }
        Ok(Err(e)) => {
            let err = eyre::eyre!("{} server failed: {}", server_name, e);
            tracing::error!(server = server_name, error = %e, "gRPC server task failed");
            Err(err)
        }
        Err(e) => {
            let err = eyre::eyre!("{} server panicked: {}", server_name, e);
            tracing::error!(server = server_name, error = %e, "gRPC server task panicked");
            Err(err)
        }
    }
}

/// Helper function to await HTTP server task completion with proper error wrapping
///
/// Wraps an HTTP server `JoinHandle` to provide:
/// - Named server identification in logs
/// - Error handling for eyre::Error
/// - Graceful handling of task panics
///
/// # Arguments
/// * `handle` - The server's JoinHandle
/// * `server_name` - Human-readable name for logging
///
/// # Returns
/// Returns `Ok(())` if server completes successfully, or an error if it fails/panics
async fn await_http_server_task(
    handle: tokio::task::JoinHandle<Result<(), eyre::Error>>,
    server_name: &str,
) -> eyre::Result<()> {
    match handle.await {
        Ok(Ok(())) => {
            tracing::warn!(
                "{} server completed unexpectedly (should run forever)",
                server_name
            );
            Ok(())
        }
        Ok(Err(e)) => {
            let err = eyre::eyre!("{} server failed: {}", server_name, e);
            tracing::error!(server = server_name, error = %e, "HTTP server task failed");
            Err(err)
        }
        Err(e) => {
            let err = eyre::eyre!("{} server panicked: {}", server_name, e);
            tracing::error!(server = server_name, error = %e, "HTTP server task panicked");
            Err(err)
        }
    }
}

/// Start the node runtime
///
/// This is the primary entry point for starting an RChain node. It:
/// 1. Creates the node identifier from the TLS certificate
/// 2. Instantiates the NodeRuntime
/// 3. Runs the main node program
/// 4. Handles any unrecoverable errors
///
/// # Arguments
/// * `node_conf` - Node configuration
///
/// # Returns
/// Returns `Ok(())` on successful node shutdown, or an error if initialization fails
pub async fn start(node_conf: NodeConf) -> eyre::Result<()> {
    info!("Starting RChain node runtime...");

    // Create node identifier from certificate
    let id = node_environment::create(&node_conf).await?;

    info!("Node initialized with ID: {}", hex::encode(&id.key));

    // Create NodeRuntime instance
    let runtime = NodeRuntime::new(node_conf, id);

    // Run the main node program with error handling
    handle_unrecoverable_errors(runtime.main()).await
}

/// Handle unrecoverable errors in the node program
///
/// Catches any errors that bubble up from the main program and logs them
/// before exiting. These are errors that should not happen in a properly
/// configured environment and mean immediate termination.
///
/// # Arguments
/// * `program` - The async program to run
///
/// # Returns
/// Returns `Ok(())` on successful completion, or logs the error and exits with code 1
async fn handle_unrecoverable_errors<F>(program: F) -> eyre::Result<()>
where F: Future<Output = eyre::Result<()>> {
    match program.await {
        Ok(_) => {
            info!("Node program completed successfully. Exiting.");
            std::process::exit(0);
        }
        Err(e) => {
            tracing::error!(error = ?e, "unhandled fatal error; exiting");
            std::process::exit(1);
        }
    }
}
