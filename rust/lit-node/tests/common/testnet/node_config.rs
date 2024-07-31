use std::path::Path;

#[cfg(all(feature = "proxy_http", feature = "testing"))]
use crate::common::faults::FAULT_TEST_HTTP_CLIENT_TIMEOUT_SECS;

use super::WhichTestnet;
use ethers::types::Address;
use lit_core::utils::toml::SimpleToml;
use lit_node::config::{
    CFG_KEY_CHAIN_POLLING_INTERVAL_MS, CFG_KEY_ECDSA_ROUND_TIMEOUT,
    CFG_KEY_ENABLE_EPOCH_TRANSITIONS, CFG_KEY_ENABLE_PROXIED_HTTP_CLIENT,
    CFG_KEY_ENABLE_RATE_LIMITING, CFG_KEY_HTTP_CLIENT_TIMEOUT,
};
use tracing::info;

pub const CUSTOM_NODE_RUNTIME_CONFIG_PATH: &str = "config/test/custom_node_runtime_config.toml";

// DEFAULT VALUES
pub const ECDSA_ROUND_TIMEOUT_DEFAULT: &str = "20000";

trait SimpleTomlInsertHelper {
    fn insertstr(&mut self, section: &str, key: &str, value: &str);
    fn insert_address(&mut self, section: &str, key: &str, value: Address) {
        self.insertstr(
            section,
            key,
            &format!("0x{}", data_encoding::HEXLOWER.encode(value.as_bytes())),
        )
    }
    fn insert_bytes(&mut self, section: &str, key: &str, value: &[u8]) {
        self.insertstr(
            section,
            key,
            &format!("0x{}", data_encoding::HEXLOWER.encode(value)),
        )
    }
}

impl SimpleTomlInsertHelper for SimpleToml {
    fn insertstr(&mut self, section: &str, key: &str, value: &str) {
        self.insert(section.to_string(), key.to_string(), value.to_string());
    }
}

#[must_use]
pub struct CustomNodeRuntimeConfigBuilder {
    ecdsa_round_timeout: Option<String>,
    enable_rate_limiting: Option<String>,
    chain_polling_interval: Option<String>,
}

impl CustomNodeRuntimeConfigBuilder {
    pub fn new() -> Self {
        Self {
            ecdsa_round_timeout: None,
            enable_rate_limiting: None,
            chain_polling_interval: None,
        }
    }

    pub fn ecdsa_round_timeout(mut self, ecdsa_round_timeout: String) -> Self {
        self.ecdsa_round_timeout = Some(ecdsa_round_timeout);
        self
    }

    pub fn enable_rate_limiting(mut self, enable_rate_limiting: String) -> Self {
        self.enable_rate_limiting = Some(enable_rate_limiting);
        self
    }

    pub fn chain_polling_interval(mut self, chain_polling_interval: String) -> Self {
        self.chain_polling_interval = Some(chain_polling_interval);
        self
    }

    pub fn build(self) -> CustomNodeRuntimeConfig {
        CustomNodeRuntimeConfig {
            ecdsa_round_timeout: self.ecdsa_round_timeout,
            enable_rate_limiting: self.enable_rate_limiting,
            chain_polling_interval: self.chain_polling_interval,
        }
    }
}

#[derive(Default)]
pub struct CustomNodeRuntimeConfig {
    ecdsa_round_timeout: Option<String>,
    enable_rate_limiting: Option<String>,
    chain_polling_interval: Option<String>,
}

impl CustomNodeRuntimeConfig {
    pub fn builder() -> CustomNodeRuntimeConfigBuilder {
        CustomNodeRuntimeConfigBuilder::new()
    }
}

/// This method is used to generate a TOML config file with custom
/// config parameters that will get merged with the rest of the node
/// config that is generated by our contract deployment tool.
pub fn generate_custom_node_runtime_config(
    is_fault_test: bool,
    which_testnet: &WhichTestnet,
    custom_config: &CustomNodeRuntimeConfig,
) {
    let advance_epoch = which_testnet.clone() != WhichTestnet::NoChain;

    let mut cfg = SimpleToml::new();

    // section node
    let section = "node";
    cfg.insertstr(
        section,
        CFG_KEY_ENABLE_PROXIED_HTTP_CLIENT,
        &is_fault_test.to_string(),
    );
    cfg.insertstr(
        section,
        CFG_KEY_ENABLE_EPOCH_TRANSITIONS,
        &advance_epoch.to_string(),
    );

    #[cfg(all(feature = "proxy_http", feature = "testing"))]
    {
        if is_fault_test {
            cfg.insertstr(
                section,
                CFG_KEY_HTTP_CLIENT_TIMEOUT,
                &FAULT_TEST_HTTP_CLIENT_TIMEOUT_SECS.to_string(),
            );
            cfg.insertstr(
                section,
                CFG_KEY_ECDSA_ROUND_TIMEOUT,
                // Multiply by 2 to account for retries.
                // Multiply by 1000 because CFG_KEY_ECDSA_ROUND_TIMEOUT should be in ms but FAULT_TEST_HTTP_CLIENT_TIMEOUT_SECS is in seconds
                &(FAULT_TEST_HTTP_CLIENT_TIMEOUT_SECS * 2 * 1000).to_string(),
            );
        }
    }

    #[cfg(not(all(feature = "proxy_http", feature = "testing")))]
    {
        cfg.insertstr(
            section,
            CFG_KEY_ECDSA_ROUND_TIMEOUT,
            &custom_config
                .ecdsa_round_timeout
                .clone()
                .unwrap_or(ECDSA_ROUND_TIMEOUT_DEFAULT.into()),
        );
        // set http timeout to 10 seconds instead of the default of 30s
        cfg.insertstr(section, CFG_KEY_HTTP_CLIENT_TIMEOUT, "10");
    }

    if let Some(chain_polling_interval) = custom_config.chain_polling_interval.clone() {
        cfg.insertstr(
            section,
            CFG_KEY_CHAIN_POLLING_INTERVAL_MS,
            &chain_polling_interval,
        );
    } else {
        cfg.insertstr(section, CFG_KEY_CHAIN_POLLING_INTERVAL_MS, "1000");
    }

    cfg.insertstr(
        section,
        CFG_KEY_ENABLE_RATE_LIMITING,
        &custom_config
            .enable_rate_limiting
            .clone()
            .unwrap_or("false".into()),
    );

    // Write file
    cfg.write_file(Path::new(CUSTOM_NODE_RUNTIME_CONFIG_PATH))
        .expect("Failed to write custom node runtime config");
    info!(
        "Generated custom node runtime config at {}",
        CUSTOM_NODE_RUNTIME_CONFIG_PATH
    );
}
