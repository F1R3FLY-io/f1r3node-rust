// See comm/src/main/scala/coop/rchain/comm/rp/Connect.scala

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::rust::errors::CommError;
use crate::rust::peer_node::PeerNode;

#[derive(Debug, Clone)]
pub struct RPConf {
    pub local: PeerNode,
    pub network_id: String,
    pub bootstrap: Option<PeerNode>,
    pub default_timeout: Duration,
    pub max_num_of_connections: usize,
    pub clear_connections: ClearConnectionsConf,
}

impl RPConf {
    pub fn new(
        local: PeerNode,
        network_id: String,
        bootstrap: Option<PeerNode>,
        default_timeout: Duration,
        max_num_of_connections: usize,
        num_of_connections_pinged: usize,
    ) -> Self {
        Self {
            local,
            network_id,
            bootstrap,
            default_timeout,
            max_num_of_connections,
            clear_connections: ClearConnectionsConf::new(num_of_connections_pinged),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClearConnectionsConf {
    pub num_of_connections_pinged: usize,
}

impl ClearConnectionsConf {
    pub fn new(num_of_connections_pinged: usize) -> Self {
        Self {
            num_of_connections_pinged,
        }
    }
}

/// Cell wrapper for RPConf to allow shared mutable access
/// Follows the same pattern as ConnectionsCell
#[derive(Clone)]
pub struct RPConfCell {
    conf: Arc<Mutex<RPConf>>,
}

impl RPConfCell {
    /// Create a new RPConfCell wrapping the given configuration
    pub fn new(conf: RPConf) -> Self {
        Self {
            conf: Arc::new(Mutex::new(conf)),
        }
    }

    /// Read the current RPConf
    pub fn read(&self) -> Result<RPConf, CommError> {
        self.conf.lock().map(|conf| conf.clone()).map_err(|_| {
            CommError::InternalCommunicationError("RPConfCell lock poisoned".to_string())
        })
    }

    /// Update the local peer node
    pub fn update_local(&self, new_local: PeerNode) -> Result<(), CommError> {
        self.conf
            .lock()
            .map(|mut conf| {
                conf.local = new_local;
            })
            .map_err(|_| {
                CommError::InternalCommunicationError("RPConfCell lock poisoned".to_string())
            })
    }

    /// Modify the entire RPConf using a transformation function
    pub fn modify<F>(&self, f: F) -> Result<RPConf, CommError>
    where F: FnOnce(RPConf) -> Result<RPConf, CommError> {
        let mut conf = self.conf.lock().map_err(|_| {
            CommError::InternalCommunicationError("RPConfCell lock poisoned".to_string())
        })?;

        let current = conf.clone();
        let new_conf = f(current)?;
        *conf = new_conf.clone();

        Ok(new_conf)
    }
}

#[cfg(test)]
mod tests {
    use prost::bytes::Bytes;

    use super::*;
    use crate::rust::peer_node::{Endpoint, NodeIdentifier, PeerNode};

    fn peer(name: &str, host: &str) -> PeerNode {
        PeerNode {
            id: NodeIdentifier {
                key: Bytes::from(name.as_bytes().to_vec()),
            },
            endpoint: Endpoint::new(host.to_string(), 40400, 40404),
        }
    }

    fn conf() -> RPConf {
        RPConf::new(
            peer("local", "localhost"),
            "testnet".to_string(),
            Some(peer("bootstrap", "bootstrap-host")),
            Duration::from_secs(3),
            25,
            7,
        )
    }

    #[test]
    fn new_populates_all_fields() {
        let c = conf();
        assert_eq!(c.local, peer("local", "localhost"));
        assert_eq!(c.network_id, "testnet");
        assert_eq!(c.bootstrap, Some(peer("bootstrap", "bootstrap-host")));
        assert_eq!(c.default_timeout, Duration::from_secs(3));
        assert_eq!(c.max_num_of_connections, 25);
        assert_eq!(c.clear_connections.num_of_connections_pinged, 7);
    }

    #[test]
    fn cell_read_returns_stored_conf() {
        let cell = RPConfCell::new(conf());
        let read = cell.read().unwrap();
        assert_eq!(read.network_id, "testnet");
        assert_eq!(read.local, peer("local", "localhost"));
    }

    #[test]
    fn cell_update_local_replaces_only_local() {
        let cell = RPConfCell::new(conf());
        cell.update_local(peer("local", "changed-host")).unwrap();

        let read = cell.read().unwrap();
        assert_eq!(read.local, peer("local", "changed-host"));
        assert_eq!(read.network_id, "testnet");
        assert_eq!(read.bootstrap, Some(peer("bootstrap", "bootstrap-host")));
    }

    #[test]
    fn cell_modify_applies_transformation_and_persists() {
        let cell = RPConfCell::new(conf());
        let returned = cell
            .modify(|mut c| {
                c.network_id = "othernet".to_string();
                c.max_num_of_connections = 1;
                Ok(c)
            })
            .unwrap();

        assert_eq!(returned.network_id, "othernet");
        assert_eq!(returned.max_num_of_connections, 1);

        let read = cell.read().unwrap();
        assert_eq!(read.network_id, "othernet");
        assert_eq!(read.max_num_of_connections, 1);
    }

    #[test]
    fn cell_modify_error_leaves_conf_unchanged() {
        let cell = RPConfCell::new(conf());
        let result = cell.modify(|_| Err(CommError::TimeOut));
        assert_eq!(result.unwrap_err(), CommError::TimeOut);
        assert_eq!(cell.read().unwrap().network_id, "testnet");
    }
}
