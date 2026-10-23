use prost::Message;

fn versioned_bytes_unchecked(deploy: &Signed<DeployData>) -> ByteBuffer {
    let mut bytes = b"F1R3DEP\x01".to_vec();
    DeployData::to_proto_ref(deploy).encode(&mut bytes).unwrap();
    bytes
}

fn try_open_pool(
    name: &str,
    live: &Arc<dyn KeyValueStore>,
    quarantine: &Arc<dyn KeyValueStore>,
) -> Result<Pool, KvStoreError> {
    match name {
        DEPLOY_STORAGE_DB => {
            KeyValueDeployStorage::from_stores(live.clone(), quarantine.clone()).map(Pool::Storage)
        }
        _ => KeyValueRejectedDeployBuffer::from_stores(live.clone(), quarantine.clone())
            .map(Pool::Buffer),
    }
}

#[test]
fn verified_open_quarantines_historical_records_with_invalid_signatures() {
    for name in POOLS {
        let (live, quarantine) = (in_memory(), in_memory());
        let valid = deploy(1);
        let mut tampered = deploy(2);
        tampered.data.phlo_limit += 1;
        let key = key_for(&tampered.sig);
        let bytes = versioned_bytes_unchecked(&tampered);
        live.put(vec![
            (
                key_for(&valid.sig),
                crate::rust::deploy::deploy_codec::encode(&valid).unwrap(),
            ),
            (key.clone(), bytes.clone()),
        ])
        .unwrap();

        let pool = Pool::open(name, &live, &quarantine);
        assert_eq!(live.get_one(&key).unwrap(), None, "{name}");
        assert_eq!(
            quarantine.get_one(&key).unwrap(),
            Some(bytes.clone()),
            "{name}"
        );
        assert_eq!(pool.read_all().unwrap(), HashSet::from([valid.clone()]));
        drop(pool);

        let reopened = Pool::open(name, &live, &quarantine);
        assert_eq!(reopened.read_all().unwrap(), HashSet::from([valid]));
        assert_eq!(live.get_one(&key).unwrap(), None, "{name}");
        assert_eq!(quarantine.get_one(&key).unwrap(), Some(bytes), "{name}");
    }
}

#[test]
fn verified_restore_keeps_invalid_signatures_and_restores_valid_records() {
    for name in POOLS {
        let (live, quarantine) = (in_memory(), in_memory());
        let valid = deploy(1);
        let mut tampered = deploy(2);
        tampered.data.phlo_limit += 1;
        let key = key_for(&tampered.sig);
        let bytes = versioned_bytes_unchecked(&tampered);
        quarantine
            .put(vec![
                (
                    key_for(&valid.sig),
                    crate::rust::deploy::deploy_codec::encode(&valid).unwrap(),
                ),
                (key.clone(), bytes.clone()),
            ])
            .unwrap();

        let pool = Pool::open(name, &live, &quarantine);
        assert_eq!(pool.read_all().unwrap(), HashSet::from([valid.clone()]));
        assert_eq!(live.get_one(&key).unwrap(), None, "{name}");
        assert_eq!(quarantine.get_one(&key).unwrap(), Some(bytes), "{name}");
        assert_eq!(quarantine.get_one(&key_for(&valid.sig)).unwrap(), None);
    }
}

#[test]
fn verified_open_preserves_legacy_and_parameterized_deploys() {
    use models::rust::deploy_parameters::{DeployParameter, RholangValue};

    for name in POOLS {
        let (live, quarantine) = (in_memory(), in_memory());
        let legacy = legacy_deploy();
        let mut data = deploy(3).data;
        data.parameters.push(DeployParameter {
            name: "input".to_string(),
            value: RholangValue::Int(42),
        });
        let parameterized = signed(data);
        live.put(vec![
            (key_for(&legacy.sig), legacy_bytes()),
            (
                key_for(&parameterized.sig),
                crate::rust::deploy::deploy_codec::encode(&parameterized).unwrap(),
            ),
        ])
        .unwrap();

        let pool = Pool::open(name, &live, &quarantine);
        assert_eq!(
            pool.read_all().unwrap(),
            HashSet::from([legacy, parameterized])
        );
        assert!(!quarantine.non_empty().unwrap());
    }
}

#[test]
fn verified_open_refuses_invalid_signatures_when_quarantine_has_conflicting_bytes() {
    for name in POOLS {
        let (live, quarantine) = (in_memory(), in_memory());
        let mut tampered = deploy(2);
        tampered.data.phlo_limit += 1;
        let key = key_for(&tampered.sig);
        let bytes = versioned_bytes_unchecked(&tampered);
        live.put_one(key.clone(), bytes.clone()).unwrap();
        quarantine.put_one(key.clone(), vec![0xee]).unwrap();

        assert!(matches!(
            try_open_pool(name, &live, &quarantine),
            Err(KvStoreError::SerializationError(_))
        ));
        assert_eq!(live.get_one(&key).unwrap(), Some(bytes), "{name}");
        assert_eq!(
            quarantine.get_one(&key).unwrap(),
            Some(vec![0xee]),
            "{name}"
        );
    }
}

#[test]
fn verified_open_propagates_backend_scan_failures() {
    for name in POOLS {
        let live: Arc<dyn KeyValueStore> = FailingStore::new(true, false);
        let quarantine = in_memory();
        assert!(matches!(
            try_open_pool(name, &live, &quarantine),
            Err(KvStoreError::IoError(_))
        ));
        assert!(!quarantine.non_empty().unwrap());
    }
}

#[test]
fn verified_open_keeps_invalid_bytes_when_the_quarantine_write_fails() {
    for name in POOLS {
        let live = in_memory();
        let quarantine: Arc<dyn KeyValueStore> = FailingStore::new(false, true);
        let mut tampered = deploy(2);
        tampered.data.phlo_limit += 1;
        let key = key_for(&tampered.sig);
        let bytes = versioned_bytes_unchecked(&tampered);
        live.put_one(key.clone(), bytes.clone()).unwrap();

        assert!(matches!(
            try_open_pool(name, &live, &quarantine),
            Err(KvStoreError::IoError(_))
        ));
        assert_eq!(live.get_one(&key).unwrap(), Some(bytes), "{name}");
        assert!(!quarantine.non_empty().unwrap());
    }
}
