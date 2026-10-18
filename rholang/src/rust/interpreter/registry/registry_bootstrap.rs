// See rholang/src/main/scala/coop/rchain/rholang/interpreter/registry/RegistryBootstrap.scala

use std::collections::BTreeMap;

use models::rhoapi::{New, Par, Receive, ReceiveBind, Send};
use models::rust::utils::{new_boundvar_par, new_freevar_par};

use crate::rust::interpreter::system_processes::FixedChannels;

pub fn ast() -> Par {
    Par::default().with_news(vec![
        bootstrap(FixedChannels::reg_lookup()),
        bootstrap(FixedChannels::reg_insert_random()),
        bootstrap(FixedChannels::reg_insert_signed()),
    ])
}

/**
 * This is used to get a one-time hold of write-only-bundled fixed
 * channel, e.g `FixedChannels.REG_LOOKUP`, from within Rholang code.
 * It can be used to produce a contract, e.g. the registry lookup
 * contract, on a write-only-bundled fixed channel.
 */
fn bootstrap(channel: Par) -> New {
    New {
        bind_count: 1,
        // for (x <- channel) { x!(channel) }
        p: Some(Par::default().with_receives(vec![Receive {
            // for (x <- channel)
            binds: vec![ReceiveBind {
                patterns: vec![new_freevar_par(0, Vec::new())],
                source: Some(channel.clone()),
                remainder: None,
                free_count: 1,
                cost_region: None,
            }],
            // x!(channel)
            body: Some(Par::default().with_sends(vec![Send {
                chan: Some(new_boundvar_par(0, Vec::new(), false)),
                data: vec![channel],
                persistent: false,
                locally_free: Vec::new(),
                connective_used: false,
                cost_region: None,
            }])),
            persistent: false,
            peek: false,
            bind_count: 1,
            locally_free: Vec::new(),
            connective_used: false,
            condition: None,
            cost_region: None,
        }])),
        uri: Vec::new(),
        injections: BTreeMap::default(),
        locally_free: Vec::new(),
    }
}
