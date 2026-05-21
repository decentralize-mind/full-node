use blockchain::{actor::BlockchainHandle, network::NetworkNode, wallet::Wallet};
use tokio::time::{interval, Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let blockchain = BlockchainHandle::start(2048);

    let mut alice_wallet = Wallet::new();
    let mut bob_wallet = Wallet::new();
    let validator_wallet = Wallet::new();

    blockchain
        .bootstrap_balance(alice_wallet.address.clone(), 1000.0)
        .await?;
    alice_wallet.balance = blockchain.get_balance(alice_wallet.address.clone()).await?;

    println!("=== Blockchain Node Started ===");
    println!("Alice's address: {}", alice_wallet.address);
    println!("Alice's balance: {}", alice_wallet.balance);
    println!("Bob's address:   {}", bob_wallet.address);

    let tx = alice_wallet.create_transaction(bob_wallet.address.clone(), 100.0, 0.001)?;

    println!("\n[TX] Alice -> Bob: 100.0 (fee: 0.001)");
    println!("     TX ID: {}", tx.id);

    // Keep behavior from existing demo (bypass strict tx verification path)
    blockchain.submit_transaction_unchecked(tx).await?;

    blockchain
        .stake_tokens(validator_wallet.address.clone(), 100.0)
        .await?;
    println!("\n[STAKE] Validator staked 100.0 tokens");
    println!("        Validator address: {}", validator_wallet.address);

    if let Some(block) = blockchain.mine_pending_block(10).await? {
        println!("\n[CONSENSUS] Validator selected: {}", block.validator);
        println!(
            "[MINING] Mining block with difficulty {}...",
            block.difficulty
        );
        println!("[MINING] Block mined! Hash: {}", block.hash);
        println!("         Nonce: {}", block.nonce);
    }

    alice_wallet.balance = blockchain.get_balance(alice_wallet.address.clone()).await?;
    bob_wallet.balance = blockchain.get_balance(bob_wallet.address.clone()).await?;

    let chain_len = blockchain.get_chain_length().await?;
    let chain_valid = blockchain.is_chain_valid().await?;

    println!("\n=== Results ===");
    println!("Blockchain length:    {} blocks", chain_len);
    println!("Chain valid:          {}", chain_valid);
    println!("Alice's new balance:  {}", alice_wallet.balance);
    println!("Bob's new balance:    {}", bob_wallet.balance);

    println!("\n[NETWORK] Starting P2P node on 127.0.0.1:8080 ...");
    let network_address = "127.0.0.1:8080".to_string();
    let mut node = NetworkNode::new(network_address, blockchain.clone());

    let actor_metrics = blockchain.metrics();
    let network_metrics = node.metrics();

    tokio::spawn(async move {
        if let Err(e) = node.start().await {
            eprintln!("Network error: {}", e);
        }
    });

    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(5));
        let mut prev_actor_processed = 0u64;
        let mut prev_net_inbound = 0u64;

        loop {
            ticker.tick().await;

            let actor = actor_metrics.snapshot();
            let net = network_metrics.snapshot();

            let actor_delta = actor.processed_total.saturating_sub(prev_actor_processed);
            let inbound_delta = net.inbound_messages_total.saturating_sub(prev_net_inbound);

            prev_actor_processed = actor.processed_total;
            prev_net_inbound = net.inbound_messages_total;

            let actor_mps = actor_delta as f64 / 5.0;
            let inbound_mps = inbound_delta as f64 / 5.0;

            println!(
                "[METRICS] actor_queue={} actor_msg/s={:.2} actor_avg_latency_ms={:.3} | val_queue={} inbound_msg/s={:.2} validated_ok={} validated_fail={} submitted={} avg_validation_ms={:.3}",
                actor.queue_depth,
                actor_mps,
                actor.avg_latency_ms,
                net.validation_queue_depth,
                inbound_mps,
                net.validation_passed_total,
                net.validation_failed_total,
                net.submitted_total,
                net.avg_validation_latency_ms
            );
        }
    });

    println!("[NETWORK] Node running. Press Ctrl+C to stop.");
    tokio::signal::ctrl_c().await?;
    println!("\n[SHUTDOWN] Node stopped.");

    Ok(())
}
