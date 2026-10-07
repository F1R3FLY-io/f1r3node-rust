// See comm/src/test/scala/coop/rchain/p2p/URIParseSpec.scala

use comm::rust::errors::CommError;
use comm::rust::peer_node::{Endpoint, NodeIdentifier, PeerNode};

fn bad_address_error(s: &str) -> Result<PeerNode, CommError> {
    Err(CommError::ParseError(format!("bad address: {}", s)))
}

#[cfg(test)]
mod tests {
    use prost::bytes::Bytes;

    use super::*;

    #[test]
    fn well_formed_rnode_uri_should_parse_into_peer_node() {
        let id = "1e780e5dfbe0a3d9470a2b414f502d59402e09c2";
        let uri = format!("rnode://{id}@localhost?protocol=12345&discovery=12346");
        let result = PeerNode::from_address(&uri);

        let expected = PeerNode {
            id: NodeIdentifier {
                key: Bytes::from(hex::decode(id).unwrap()),
            },
            endpoint: Endpoint {
                host: "localhost".to_string(),
                tcp_port: 12345,
                udp_port: 12346,
            },
        };

        assert!(result.is_ok());
        let peer = result.unwrap();
        assert_eq!(peer.id.key, expected.id.key);
        assert_eq!(peer.endpoint.host, expected.endpoint.host);
        assert_eq!(peer.endpoint.tcp_port, expected.endpoint.tcp_port);
        assert_eq!(peer.endpoint.udp_port, expected.endpoint.udp_port);
    }

    #[test]
    fn non_rnode_uri_should_parse_as_error() {
        let uri = "http://foo.bar.baz/quux";
        let result = PeerNode::from_address(uri);
        let expected = bad_address_error(uri);

        assert!(result.is_err());
        match (&result, &expected) {
            (Err(CommError::ParseError(_)), Err(CommError::ParseError(_))) => (),
            _ => panic!("Expected ParseError for both result and expected"),
        }
    }

    #[test]
    fn uri_without_protocol_should_parse_as_error() {
        let uri = "abcde@localhost?protocol=12345&discovery=12346";
        let result = PeerNode::from_address(uri);
        let expected = bad_address_error(uri);

        assert!(result.is_err());
        match (&result, &expected) {
            (Err(CommError::ParseError(_)), Err(CommError::ParseError(_))) => (),
            _ => panic!("Expected ParseError for both result and expected"),
        }
    }

    #[test]
    fn rnode_uri_with_non_integral_port_should_parse_as_error() {
        let uri = "rnode://abcde@localhost:smtp";
        let result = PeerNode::from_address(uri);
        let expected = bad_address_error(uri);

        assert!(result.is_err());
        match (&result, &expected) {
            (Err(CommError::ParseError(_)), Err(CommError::ParseError(_))) => (),
            _ => panic!("Expected ParseError for both result and expected"),
        }
    }

    #[test]
    fn malformed_node_ids_should_parse_as_error() {
        for id in [
            "0000000000000000000000000000000000000000",
            "zzzz",
            "abcdef",
            "1e780e5dfbe0a3d9470a2b414f502d59402e09c2ff",
        ] {
            let uri = format!("rnode://{id}@localhost?protocol=12345&discovery=12346");
            assert!(
                matches!(PeerNode::from_address(&uri), Err(CommError::ParseError(_))),
                "node id {id:?} must be rejected"
            );
        }
    }

    #[test]
    fn rnode_uri_without_key_should_parse_as_error() {
        let uri = "rnode://localhost?protocol=12345&discovery=12346";
        let result = PeerNode::from_address(uri);
        let expected = bad_address_error(uri);

        assert!(result.is_err());
        match (&result, &expected) {
            (Err(CommError::ParseError(_)), Err(CommError::ParseError(_))) => (),
            _ => panic!("Expected ParseError for both result and expected"),
        }
    }
}
