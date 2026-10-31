use serde::{Deserialize, Serialize};

pub trait PriceHook: Send + Sync {
    fn acceptance_price(&self) -> i64;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct NodePriceConf {
    pub fixed_price: i64,
}

impl NodePriceConf {
    pub fn check(&self) -> Result<(), String> {
        if self.fixed_price < 0 {
            return Err(format!(
                "the node's fixed acceptance price {} must be non-negative",
                self.fixed_price
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixedPriceHook {
    price: i64,
}

impl FixedPriceHook {
    pub fn from_conf(conf: &NodePriceConf) -> Result<Self, String> {
        conf.check()?;
        Ok(FixedPriceHook {
            price: conf.fixed_price,
        })
    }
}

impl PriceHook for FixedPriceHook {
    fn acceptance_price(&self) -> i64 { self.price }
}

pub fn effective_price(hook: &dyn PriceHook, shard_floor: i64) -> i64 {
    hook.acceptance_price().max(shard_floor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fixed_hook_returns_the_configured_price() {
        let hook = FixedPriceHook::from_conf(&NodePriceConf { fixed_price: 7 }).unwrap();
        assert_eq!(hook.acceptance_price(), 7);
        assert!(FixedPriceHook::from_conf(&NodePriceConf { fixed_price: -1 }).is_err());
    }

    #[test]
    fn the_node_never_accepts_below_the_shard_floor() {
        let hook = FixedPriceHook::from_conf(&NodePriceConf { fixed_price: 3 }).unwrap();
        assert_eq!(effective_price(&hook, 1), 3);
        assert_eq!(effective_price(&hook, 5), 5);
    }

    #[test]
    fn the_configuration_reads_kebab_case_with_a_default() {
        let conf: NodePriceConf = serde_json::from_str(r#"{"fixed-price": 12}"#).unwrap();
        assert_eq!(conf.fixed_price, 12);
        let conf: NodePriceConf = serde_json::from_str("{}").unwrap();
        assert_eq!(conf, NodePriceConf::default());
    }
}
