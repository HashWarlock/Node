use std::{io::BufRead, time::Duration};

use crate::common::node_collection::hit_ports_with_json_body_join_all;
use crate::common::testnet::actions::Actions;
use crate::common::testnet::Testnet;
use crate::common::validator::ValidatorCollection;
use crate::common::{
    self,
    auth_sig::generate_authsig_item,
    pkp::{
        generate_data_to_send, send_signing_requests, sign_bytes_with_pkp, sign_message_with_pkp,
    },
};
use common::{
    new_node_collection,
    pkp::{add_permitted_address_to_pkp, mint_next_pkp},
};
use ethers::signers::{LocalWallet, Signer};
use ethers::{core::types::Address, types::TransactionRequest};
use ethers::{providers::Middleware, signers::to_eip155_v};
use ethers::{types::U256, utils::keccak256};
use futures::future::join_all;
use lit_core::utils::binary::{bytes_to_hex, hex_to_bytes};
use lit_node::{models::JsonPKPSigningRequest, utils::web::pubkey_bytes_to_eth_address_bytes};
use rand::Rng;
use serde_json::Value;
use tracing::{error, info, warn};

#[tokio::test]
#[ignore]
async fn test_pkp_permissions_get_address_registered() {
    common::init_test_config();
    let num_nodes = 3;

    let (_testnet, validator_collection) = new_node_collection(num_nodes, false).await;

    let permitted_pubkey = "0x5aaeC3Bd77f1F05f7B1C36927CDc4DB24Ec95bFc";
    let res = common::pkp::generate_pkp_check_get_permitted_address(
        permitted_pubkey,
        &validator_collection,
    )
    .await;

    assert!(res.is_ok());
    let res = res.unwrap();

    info!("get permitted address result: {:?}", res);
    assert!(!res.is_empty());

    // check second address as first address is itself
    assert!(res[1] == String::from(permitted_pubkey).to_lowercase());
}

#[tokio::test]
#[ignore]
async fn test_pkp_permissions_is_address_registered() {
    common::init_test_config();
    let num_nodes = 3;

    let (_testnet, validator_collection) = new_node_collection(num_nodes, false).await;
    let permitted_pubkey = "0x5aaeC3Bd77f1F05f7B1C36927CDc4DB24Ec95bFc";
    let res = common::pkp::generate_pkp_check_is_permitted_address(
        permitted_pubkey,
        validator_collection.actions(),
    )
    .await;

    assert!(res.0.is_ok());
    let res = res.0.unwrap();

    info!("get permitted address result: {:?}", res);
    assert!(res);
}

#[tokio::test]
#[doc = "Test that a signature generated by a PKP can create a valid eth txn"]
pub async fn test_pkp_hd_sign_and_submit_eth_txn() {
    common::init_test_config();
    info!("Starting test: test_pkp_hd_sign_and_submit_eth_txn");
    let num_nodes = 3;
    let (testnet, validator_collection) = new_node_collection(num_nodes, false).await;
    // first, mint a PKP
    let minted_key = mint_next_pkp(validator_collection.actions()).await;
    if let Err(e) = minted_key {
        panic!("Failed to mint key: {:?}", e);
    }
    assert!(minted_key.is_ok());
    let minted_key = minted_key.unwrap();
    let pubkey = minted_key.0;
    let token_id = minted_key.1;

    // get pkp address from chain
    let pkp_address_from_chain = validator_collection
        .actions()
        .contracts()
        .pkpnft
        .get_eth_address(token_id)
        .await
        .expect("Could not get pkp address from chain");

    // send gas to the PKP
    let pkp_address = Address::from_slice(
        &pubkey_bytes_to_eth_address_bytes(
            hex_to_bytes(&pubkey).expect("Could not convert pubkey string to bytes"),
        )
        .expect("Could not convert pubkey to eth address"),
    );
    info!("PKP Ethereum address: {:?}", pkp_address);
    assert!(pkp_address == pkp_address_from_chain);
    let signer = testnet.deploy_account.signing_provider.clone();
    let pkp_balance_before = signer
        .get_balance(pkp_address, None)
        .await
        .expect("Could not get PKP balance before");
    assert!(pkp_balance_before == U256::from(0));
    let tx = TransactionRequest::new()
        .to(pkp_address)
        .value(1000000000000000000_u64)
        .from(testnet.deploy_address);
    let tx_hash = signer
        .send_transaction(tx, None)
        .await
        .expect("Could not send eth txn");
    let _receipt = tx_hash.await.expect("Could not get receipt");
    let pkp_balance_after = signer
        .get_balance(pkp_address, None)
        .await
        .expect("Could not get PKP Balance after");
    assert!(pkp_balance_after == U256::from(1000000000000000000_u64));

    // generate an eth txn to sign
    let value_to_send = 10;
    let tx = TransactionRequest::new()
        .to(signer.address())
        .value(value_to_send)
        .from(pkp_address)
        .gas(21000)
        .gas_price(1000000000_u64)
        .chain_id(31337)
        .nonce(0)
        .data(vec![]);
    let to_sign_as_sighash = tx.sighash();
    let to_sign = to_sign_as_sighash.0.to_vec();

    info!("pkp balance is {}", pkp_balance_after);
    info!(
        "gas * price + value = {}",
        tx.gas.unwrap() * tx.gas_price.unwrap() + tx.value.unwrap()
    );

    // 1000000000000000000
    // 21000000000010

    let result = sign_bytes_with_pkp(
        validator_collection.actions(),
        pubkey.clone(),
        to_sign.clone(),
    )
    .await;
    assert!(result.is_ok());
    let result = result.unwrap();
    assert!(result.0);
    let signature = result.1;
    let recovery_id = result.2;

    // submit the signature to eth
    let deployer_balance_before_sending_from_pkp = signer
        .get_balance(testnet.deploy_address, None)
        .await
        .expect("Could not get deployer balance before sending from PKP");

    let r_bytes = signature.r().to_bytes();
    let r = r_bytes.as_slice();
    let s_bytes = signature.s().to_bytes();
    let s = s_bytes.as_slice();
    let v = recovery_id.to_byte();
    info!("v is {}", v);
    let ethers_signature = ethers::types::Signature {
        r: U256::from_big_endian(r),
        s: U256::from_big_endian(s),
        v: to_eip155_v(v, 31337),
    };
    info!("Ethers signature: {:?}", ethers_signature);
    // check that the signature matches the pubkey
    let recovered = ethers_signature.recover(to_sign_as_sighash).unwrap();
    info!("Proper PKP address: {:?}", pkp_address);
    info!("Recovered address: {:?}", recovered);
    assert!(recovered == pkp_address);

    let rlp_signed = tx.rlp_signed(&ethers_signature);
    let tx_hash = signer
        .send_raw_transaction(rlp_signed)
        .await
        .expect("Could not send txn for PKP");
    let _receipt = tx_hash.await.expect("Could not get receipt");
    let pkp_balance_after_sending_from_pkp = signer
        .get_balance(pkp_address, None)
        .await
        .expect("Could not get PKP Balance after sending from PKP");
    assert!(pkp_balance_after_sending_from_pkp <= pkp_balance_after - value_to_send);

    let deployer_balance_after_sending_from_pkp = signer
        .get_balance(testnet.deploy_address, None)
        .await
        .expect("Could not get deployer balance after sending from PKP");
    assert!(
        deployer_balance_after_sending_from_pkp
            == deployer_balance_before_sending_from_pkp + value_to_send
    );

    // now test that if we try to sign something that's no 32 bytes, it will return an error.  this could be it's own test but then we have to set up the nodes again and i don't think we need to do that for such a small test
    let auth_sig = generate_authsig_item(signer.signer())
        .await
        .expect("Could not generate authsig item");

    let epoch = validator_collection.actions().get_current_epoch().await;
    let epoch = epoch.as_u64();
    let data_to_send = JsonPKPSigningRequest {
        auth_sig,
        to_sign: vec![1, 2, 3, 4],
        pubkey,
        auth_methods: None,
        epoch,
    };
    let endpoint_responses = send_signing_requests(validator_collection.actions(), data_to_send)
        .await
        .expect("Could not send signing requests");
    let json_responses: Vec<Value> = endpoint_responses
        .iter()
        .map(|x| serde_json::from_str(x).expect("Could not parse JSON response"))
        .collect();

    // result should contain an error that the length isn't 32 bytes
    let mut error_counts = 0;
    // check all responses and that we got 2 errors
    for response in json_responses {
        if !response.is_object() || !response.as_object().unwrap().contains_key("details") {
            continue;
        }
        let err_str = response["details"][0].to_string();
        // info!("checking response err string: {:?}", err_str);
        if err_str.contains("Message length to be signed is not 32 bytes.") {
            error_counts += 1;
        } else {
            info!(
                "Error string doesn't contain 32 bytes message.  Error string is {}",
                err_str
            );
        }
    }
    assert!(
        error_counts == 2,
        "Error counts should be 2 but it is {}",
        error_counts
    );
}

#[tokio::test]
#[doc = "Primary test to ensure that the network can sign with a PKP key.  It goes through the process of spinning up the network, minting a new PKP, and then signing with it."]
pub async fn test_pkp_hd_sign_generic_key() {
    common::init_test_config();
    info!("Starting test: test_hd_pkp_sign");
    let num_nodes = 3;
    let (testnet, validator_collection, pubkey) =
        new_node_collection_with_authd_pkp(num_nodes, false).await;

    // check to see that we can sign
    assert!(
        simple_single_sign_with_hd_key(
            &validator_collection
                .ports()
                .iter()
                .map(|p| p.to_string())
                .collect(),
            validator_collection.actions(),
            &pubkey
        )
        .await,
        "Failed to sign first time with all nodes up."
    );

    drop(testnet);
}

#[tokio::test]
#[doc = "Primary test to ensure that the network can sign with a PKP key.  It goes through the process of spinning up the network, minting a new PKP, and then signing with it, advancing an epoch and signing again.."]
pub async fn test_pkp_hd_sign_generic_key_with_epoch_change() {
    common::init_test_config();

    info!("Starting test: test_pkp_hd_sign_generic_key_with_epoch_change");
    let num_nodes = 3;
    let (_testnet, validator_collection, pubkey) =
        new_node_collection_with_authd_pkp(num_nodes, false).await;

    let current_epoch = validator_collection.actions().get_current_epoch().await;

    // check to see that we can sign
    assert!(
        simple_single_sign_with_hd_key(
            &validator_collection
                .ports()
                .iter()
                .map(|p| p.to_string())
                .collect(),
            validator_collection.actions(),
            &pubkey
        )
        .await,
        "Failed to sign first time with all nodes up."
    );

    // Wait for the new node to be active.
    validator_collection.actions().wait_for_active().await;

    //in test peers refresh every 1 second, so this allows time to refresh peer data after the new node is staked and active.
    validator_collection.actions().sleep_millis(2000).await;

    // Fast forward the network by 300 seconds, and wait for the new node to be active - effectively waiting for the next epoch.
    validator_collection
        .actions()
        .increase_blockchain_timestamp(300)
        .await;

    // Wait for DKG to start and then finish, by effectively waiting for the epoch change - nodes become active once more.
    validator_collection
        .actions()
        .wait_for_epoch(current_epoch + 1)
        .await;

    // check to see that we can sign
    assert!(
        simple_single_sign_with_hd_key(
            &validator_collection
                .ports()
                .iter()
                .map(|p| p.to_string())
                .collect(),
            validator_collection.actions(),
            &pubkey
        )
        .await,
        "Failed to sign after epoch change."
    );
}

#[tokio::test]
#[doc = "Primary test to ensure that the network can sign using ECDSA when one node has dropped."]
pub async fn test_pkp_signing_when_nodes_drop() {
    common::init_test_config();
    info!("Starting test: test_pkp_signing_when_nodes_drop");
    let num_nodes = 5;
    let node_to_kill = 3;

    let (_testnet, mut validator_collection, pubkey) =
        new_node_collection_with_authd_pkp(num_nodes, false).await;

    assert!(
        simple_single_sign_with_hd_key(
            &validator_collection
                .ports()
                .iter()
                .map(|p| p.to_string())
                .collect(),
            validator_collection.actions(),
            &pubkey
        )
        .await,
        "Failed to sign with all nodes up."
    );

    assert!(validator_collection.stop_node(node_to_kill).await.is_ok());
    let current_epoch = validator_collection.actions().get_current_epoch().await;

    let staker_address_to_kick = _testnet.node_accounts[node_to_kill].staker_address;
    let staker_address_of_non_faulty_node = _testnet.node_accounts[0].staker_address;
    let get_voting_status_res = validator_collection
        .actions()
        .wait_for_voting_status_to_kick_validator(
            U256::from(2),
            staker_address_to_kick,
            staker_address_of_non_faulty_node,
            3,
        )
        .await;
    assert!(get_voting_status_res.is_ok());
    info!("Faulty node is kicked");

    // wait for next epoch to start
    validator_collection
        .actions()
        .wait_for_epoch(current_epoch + 1)
        .await;

    assert!(
        simple_single_sign_with_hd_key(
            &validator_collection
                .ports()
                .iter()
                .map(|p| p.to_string())
                .collect(),
            validator_collection.actions(),
            &pubkey
        )
        .await,
        "Failed to sign after node drops."
    );
}

pub async fn simple_single_sign_with_hd_key(
    portnames: &Vec<String>,
    actions: &Actions,
    pubkey: &str,
) -> bool {
    sign_with_hd_key(
        portnames,
        actions,
        pubkey.to_string(),
        false,
        false,
        1,
        None,
        true,
    )
    .await
}

#[tokio::test]
#[doc = "Simple Beaver Triples test."]
pub async fn test_beaver_triples() {
    common::init_test_config();
    info!("Starting test: Simple Beaver Triples test");

    let messages_to_sign = 10;
    let num_nodes = 3;
    let (_testnet, validator_collection, pubkey) =
        new_node_collection_with_authd_pkp(num_nodes, false).await;

    // open the log files
    let mut log_readers = validator_collection.log_readers();

    let start = std::time::Instant::now();
    let _ = sign_with_hd_key(
        &validator_collection
            .ports()
            .iter()
            .map(|p| p.to_string())
            .collect(),
        validator_collection.actions(),
        pubkey.clone(),
        false,
        true,
        1,
        Some("First Test message".to_string()),
        false,
    )
    .await;
    info!("Requiring'd BTs: Time elapsed: {:?}", start.elapsed());

    // give the nodes a few seconds to populate a triple or two.
    let warmup_time = Duration::from_millis(5000);
    validator_collection
        .actions()
        .sleep_millis(warmup_time.as_millis() as u64)
        .await;

    // clear the log buffer
    for reader in &mut log_readers {
        let _lines = reader
            .lines()
            .map(|line| line.unwrap_or("".to_string()))
            .collect::<Vec<String>>();
    }

    let mut bt_cache_hit = 0;
    let mut bt_cache_hit_duration: Duration = Duration::from_millis(0);
    let mut bt_cache_miss = 0;
    let mut bt_cache_miss_duration: Duration = Duration::from_millis(0);
    let mut sign_success = 0;
    let mut total_sleep = Duration::from_millis(0);
    let mut signing_time: Vec<u32> = Vec::new();
    let start = std::time::Instant::now();
    for i in 0..messages_to_sign {
        info!("Starting sig #{}", i);
        let message_to_sign = Some(format!("Test message #{}", i));
        let start_1 = std::time::Instant::now();
        let validation = sign_with_hd_key(
            &validator_collection
                .ports()
                .iter()
                .map(|p| p.to_string())
                .collect(),
            validator_collection.actions(),
            pubkey.clone(),
            false,
            false,
            1,
            message_to_sign,
            false,
        )
        .await;

        if validation {
            sign_success += 1;
        } else {
            error!("Validation failed for sig #{}", i);
        }

        let completion_time = start_1.elapsed();
        let mut node_cache_hit_count = 0;
        let mut node_cache_miss_count = 0;
        for reader in &mut log_readers {
            let lines = reader
                .lines()
                .map(|line| line.unwrap_or("".to_string()))
                .collect::<Vec<String>>();
            for line in lines {
                if line.contains("BT Cache Hit") {
                    node_cache_hit_count += 1;
                    break;
                }
                if line.contains("BT Cache Miss") {
                    node_cache_miss_count += 1;
                    break;
                }
            }
        }
        info!("node_cache_hit_count: {}", node_cache_hit_count);
        info!("node_cache_miss_count: {}", node_cache_miss_count);
        let consensus = node_cache_hit_count == num_nodes || node_cache_miss_count == num_nodes;
        if !consensus {
            error!("We did not get consensus among the nodes to tell if this was a hit or a miss.  node_cache_hit_count: {}, node_cache_miss_count: {}", node_cache_hit_count, node_cache_miss_count)
        }
        // assert!(consensus);
        if node_cache_hit_count >= node_cache_miss_count {
            bt_cache_hit += 1;
            bt_cache_hit_duration += completion_time;
        } else {
            bt_cache_miss += 1;
            bt_cache_miss_duration += completion_time;
        }
        signing_time.push(start_1.elapsed().as_millis() as u32);
        let sleep_time = rand::thread_rng().gen_range(0..100) * 50_u64;
        total_sleep += Duration::from_millis(sleep_time);
        validator_collection
            .actions()
            .sleep_millis((sleep_time) as u64)
            .await;
    }

    let total_elapsed = start.elapsed();
    info!(
        "
        Signing {} messages randomly in a {} node network 
        Pregen BT  Warmup: {:?} 
        BT Cache Hit (qty/time): {} / {:?}  
        BT Cache Miss (qty/time): {} / {:?} 
        Cache success: {:?} 
        Total time spent sleeping: {:?} 
        Total Time elapsed: {:?} 
        Sign success: {:?} 
        Signing time: {:?} ",
        messages_to_sign,
        num_nodes,
        warmup_time,
        bt_cache_hit,
        bt_cache_hit_duration,
        bt_cache_miss,
        bt_cache_miss_duration,
        (bt_cache_hit as f64 / messages_to_sign as f64),
        total_sleep,
        total_elapsed,
        sign_success,
        signing_time
    );

    assert!(
        sign_success == messages_to_sign,
        "Sign success: {}, messages_to_sign: {}",
        sign_success,
        messages_to_sign
    );
}

#[tokio::test]
#[ignore]
#[doc = "Volume Beaver Triples test, with unsafe triples."]
pub async fn test_volume_sigs_with_unsafe_beaver_triples() {
    common::init_test_config();
    info!("Starting test: test_hd_pkp_sign");
    let num_nodes = 3; // nodesin the network
    let messages_to_sign = 50; // messages tosign.
    let num_pairs = 100; // number of BTs to generate - should only be high if we are using "unsafe" triples or the test will take hours!
                         // a good rule of thumb for pairs is messages_to_sign * num_nodes * 1.5 if you want to avoid cache misses
    let start = std::time::Instant::now();

    use crate::component::beaver_triple_pairs::generate_triple_pairs;
    generate_triple_pairs(num_nodes, num_pairs, false).await;

    info!(
        "Generated {} triple pairs in {:?}",
        num_pairs,
        start.elapsed()
    );
    let (_, validator_collection, pubkey) =
        new_node_collection_with_authd_pkp(num_nodes, false).await;

    info!("Signing {} messages...", messages_to_sign);

    let start = std::time::Instant::now();
    let _validation = sign_with_hd_key(
        &validator_collection
            .ports()
            .iter()
            .map(|p| p.to_string())
            .collect(),
        validator_collection.actions(),
        pubkey.clone(),
        true,
        false,
        messages_to_sign,
        None,
        false,
    )
    .await;

    let total_elapsed = start.elapsed();
    info!(
        "
        Signing {} messages concurrently in a {} node network 
        Total Time elapsed: {:?} 
        Sign success: {:?} ",
        messages_to_sign, num_nodes, total_elapsed, 1
    );
}

#[ignore]
#[doc = "Test to validate a number of messages signed with a single key.  This function helps test out differences in rec_id or similar issues."]
pub async fn test_pkp_hd_sign_20_messages_generic_key() {
    common::init_test_config();
    info!("Starting test: test_hd_pkp_sign");
    let num_nodes = 3;
    let (_, validator_collection, pubkey) =
        new_node_collection_with_authd_pkp(num_nodes, false).await;

    let validation = sign_with_hd_key(
        &validator_collection
            .ports()
            .iter()
            .map(|p| p.to_string())
            .collect(),
        validator_collection.actions(),
        pubkey.clone(),
        false,
        false,
        20,
        None,
        true,
    )
    .await;

    assert!(validation);
}

pub async fn new_node_collection_with_authd_pkp(
    num_nodes: usize,
    is_fault_test: bool,
) -> (Testnet, ValidatorCollection, String) {
    let (testnet, validator_collection, pubkey, _token_id) =
        new_node_collection_with_authd_pkp_with_token(num_nodes, is_fault_test).await;
    (testnet, validator_collection, pubkey)
}

pub async fn new_node_collection_with_authd_pkp_with_token(
    num_nodes: usize,
    is_fault_test: bool,
) -> (Testnet, ValidatorCollection, String, U256) {
    let (testnet, validator_collection) = new_node_collection(num_nodes, is_fault_test).await;

    let minted_key = mint_next_pkp(validator_collection.actions()).await;
    if let Err(e) = minted_key {
        panic!("Failed to mint key: {:?}", e);
    }
    assert!(minted_key.is_ok());
    let minted_key = minted_key.unwrap();
    let pubkey = minted_key.0;
    let token_id = minted_key.1;

    let address_bytes = testnet.node_accounts[0].staker_address.as_bytes();
    let address = &bytes_to_hex(address_bytes);

    let res = add_permitted_address_to_pkp(
        validator_collection.actions(),
        address,
        token_id,
        &[U256::from(1)],
    )
    .await;

    assert!(res.is_ok());
    (testnet, validator_collection, pubkey, token_id)
}

#[doc = "Mint a new key and sign with it.  This is a helper function for the test_pkp_hd_sign_generic_key test."]
pub async fn sign_with_hd_key(
    portnames: &Vec<String>,
    actions: &Actions,
    pubkey: String,
    concurrent_signing: bool,
    concurrent_randomization: bool,
    messages_to_sign: i32,
    message_to_sign: Option<String>,
    assert_inline: bool,
) -> bool {
    let mut validation = false;
    let mut future_validations = Vec::new();
    let expected_responses = portnames.len();
    let max_sleep_ms = 100; // a number between 1 and size of random number generator (currently a u8) ... creates concurrency when the rnd is above this value
    for i in 0..messages_to_sign {
        let to_sign = match message_to_sign.clone() {
            Some(m) => m,
            None => format!("test message #{}", i),
        };

        info!("Testing message #{}: {:?}", i, to_sign);

        if concurrent_signing {
            let data_to_send = generate_data_to_send(
                pubkey.clone(),
                keccak256(to_sign.as_bytes()).into(),
                actions,
            )
            .await
            .expect("Failed to generate PKP Signing Request.");
            let portnames = portnames.clone();
            let json_body = serde_json::to_string(&data_to_send).unwrap();
            let cmd = "/web/pkp/sign".to_string();

            let future_sign =
                tokio::spawn(hit_ports_with_json_body_join_all(portnames, cmd, json_body));
            future_validations.push(future_sign);
            if concurrent_randomization {
                let mut sleep_time = rand::random::<u8>() as u64;
                if sleep_time > max_sleep_ms {
                    sleep_time = 0;
                }
                actions.sleep_millis(sleep_time).await;
            }
        } else {
            validation = sign_message_with_pkp(actions, pubkey.clone(), to_sign)
                .await
                .unwrap();

            if assert_inline {
                assert!(validation);
            }
        }
    }

    if concurrent_signing {
        warn!("Waiting for concurrent signing to complete.");
        let validations = join_all(future_validations).await;
        for v in validations {
            let responses = v.unwrap();

            assert!(responses.is_ok());
            let responses = responses.unwrap();
            assert!(responses.len() == expected_responses);

            validation = true;
        }
    }

    validation
}

#[tokio::test]
#[ignore]
#[doc = "This test is used to load a network for external tests.  It will run for a little more than 1 day, and is not intended to be run as part of the test suite."]
async fn load_network_for_external_tests() {
    common::init_test_config();
    let num_nodes = 3;
    let (_testnet, validator_collection, _pubkey, token_id) =
        new_node_collection_with_authd_pkp_with_token(num_nodes, false).await;

    let wallet = LocalWallet::new(&mut rand_core::OsRng);
    let address = &bytes_to_hex(&wallet.address().to_fixed_bytes());

    let res = add_permitted_address_to_pkp(
        validator_collection.actions(),
        address,
        token_id,
        &[U256::from(1)],
    )
    .await;

    info!("Started network for external tests");

    let secret = bytes_to_hex(&wallet.signer().as_nonzero_scalar().to_bytes());
    info!("Wallet address that controls a minted PKP: {}", address);
    info!("Secret that controls a minted PKP: {}", secret);

    tokio::time::sleep(std::time::Duration::from_secs(100000)).await;
}
