use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, Mutex};

use super::actor::BlockchainHandle;
use super::block::Block;
use super::transaction::Transaction;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkMessage {
    NewBlock(Block),
    NewTransaction(Transaction),
    RequestChain,
    ResponseChain(Vec<Block>),
    RequestPeers,
    ResponsePeers(Vec<String>),
    Ping,
    Pong,
}

#[derive(Debug, Clone)]
pub struct NetworkConfig {
    pub validation_workers: usize,
    pub validation_queue_size: usize,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        let cpu = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);

        Self {
            validation_workers: cpu.max(2),
            validation_queue_size: 4096,
        }
    }
}

#[derive(Debug)]
pub struct NetworkNode {
    pub address: String,
    pub peers: HashMap<String, PeerInfo>,
    pub blockchain: BlockchainHandle,
    pub config: NetworkConfig,
    metrics: NetworkMetricsHandle,
}

#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub address: String,
    pub last_seen: i64,
    pub reputation: f64,
}

#[derive(Clone, Debug)]
pub struct NetworkMetricsHandle {
    inner: Arc<NetworkMetricsInner>,
}

#[derive(Debug)]
struct NetworkMetricsInner {
    inbound_messages_total: AtomicU64,
    validation_passed_total: AtomicU64,
    validation_failed_total: AtomicU64,
    submitted_total: AtomicU64,
    validation_queue_depth: AtomicUsize,
    validation_total_ns: AtomicU64,
    validation_samples: AtomicU64,
}

#[derive(Clone, Debug)]
pub struct NetworkMetricsSnapshot {
    pub inbound_messages_total: u64,
    pub validation_passed_total: u64,
    pub validation_failed_total: u64,
    pub submitted_total: u64,
    pub validation_queue_depth: usize,
    pub avg_validation_latency_ms: f64,
}

struct ValidationJob {
    message: NetworkMessage,
}

impl NetworkMetricsHandle {
    fn new() -> Self {
        Self {
            inner: Arc::new(NetworkMetricsInner {
                inbound_messages_total: AtomicU64::new(0),
                validation_passed_total: AtomicU64::new(0),
                validation_failed_total: AtomicU64::new(0),
                submitted_total: AtomicU64::new(0),
                validation_queue_depth: AtomicUsize::new(0),
                validation_total_ns: AtomicU64::new(0),
                validation_samples: AtomicU64::new(0),
            }),
        }
    }

    fn on_message_received(&self) {
        self.inner
            .inbound_messages_total
            .fetch_add(1, Ordering::Relaxed);
    }

    fn on_validation_enqueued(&self) {
        self.inner
            .validation_queue_depth
            .fetch_add(1, Ordering::Relaxed);
    }

    fn on_validation_enqueue_failed(&self) {
        self.inner
            .validation_queue_depth
            .fetch_sub(1, Ordering::Relaxed);
    }

    fn on_validation_dequeued(&self) {
        self.inner
            .validation_queue_depth
            .fetch_sub(1, Ordering::Relaxed);
    }

    fn on_validation_result(&self, valid: bool, elapsed: std::time::Duration) {
        if valid {
            self.inner
                .validation_passed_total
                .fetch_add(1, Ordering::Relaxed);
        } else {
            self.inner
                .validation_failed_total
                .fetch_add(1, Ordering::Relaxed);
        }

        self.inner
            .validation_total_ns
            .fetch_add(elapsed.as_nanos() as u64, Ordering::Relaxed);
        self.inner
            .validation_samples
            .fetch_add(1, Ordering::Relaxed);
    }

    fn on_submitted(&self) {
        self.inner.submitted_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> NetworkMetricsSnapshot {
        let samples = self.inner.validation_samples.load(Ordering::Relaxed);
        let total_ns = self.inner.validation_total_ns.load(Ordering::Relaxed);

        NetworkMetricsSnapshot {
            inbound_messages_total: self.inner.inbound_messages_total.load(Ordering::Relaxed),
            validation_passed_total: self.inner.validation_passed_total.load(Ordering::Relaxed),
            validation_failed_total: self.inner.validation_failed_total.load(Ordering::Relaxed),
            submitted_total: self.inner.submitted_total.load(Ordering::Relaxed),
            validation_queue_depth: self.inner.validation_queue_depth.load(Ordering::Relaxed),
            avg_validation_latency_ms: if samples == 0 {
                0.0
            } else {
                (total_ns as f64 / samples as f64) / 1_000_000.0
            },
        }
    }
}

impl NetworkNode {
    pub fn new(address: String, blockchain: BlockchainHandle) -> Self {
        Self::with_config(address, blockchain, NetworkConfig::default())
    }

    pub fn with_config(
        address: String,
        blockchain: BlockchainHandle,
        config: NetworkConfig,
    ) -> Self {
        NetworkNode {
            address,
            peers: HashMap::new(),
            blockchain,
            config,
            metrics: NetworkMetricsHandle::new(),
        }
    }

    pub fn metrics(&self) -> NetworkMetricsHandle {
        self.metrics.clone()
    }

    pub async fn start(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind(&self.address).await?;
        println!("Node listening on {}", self.address);

        // Bounded validation queue + worker pool
        let (validation_tx, validation_rx) =
            mpsc::channel::<ValidationJob>(self.config.validation_queue_size);
        let shared_rx = Arc::new(Mutex::new(validation_rx));

        for _ in 0..self.config.validation_workers {
            let rx = shared_rx.clone();
            let blockchain = self.blockchain.clone();
            let metrics = self.metrics.clone();

            tokio::spawn(async move {
                loop {
                    let next_job = {
                        let mut locked = rx.lock().await;
                        locked.recv().await
                    };

                    let Some(job) = next_job else {
                        break;
                    };

                    metrics.on_validation_dequeued();
                    let started = Instant::now();
                    let valid = Self::validate_message(&job.message);
                    metrics.on_validation_result(valid, started.elapsed());

                    if !valid {
                        continue;
                    }

                    if Self::submit_validated_message(job.message, blockchain.clone())
                        .await
                        .is_ok()
                    {
                        metrics.on_submitted();
                    }
                }
            });
        }

        loop {
            let (mut socket, _addr) = listener.accept().await?;
            let validation_tx = validation_tx.clone();
            let metrics = self.metrics.clone();

            tokio::spawn(async move {
                let mut buf = vec![0; 1024];
                loop {
                    match socket.read(&mut buf).await {
                        Ok(n) if n == 0 => break,
                        Ok(n) => {
                            if let Ok(message) = serde_json::from_slice::<NetworkMessage>(&buf[..n])
                            {
                                metrics.on_message_received();
                                metrics.on_validation_enqueued();
                                let send_result =
                                    validation_tx.send(ValidationJob { message }).await;

                                if send_result.is_err() {
                                    metrics.on_validation_enqueue_failed();
                                    break;
                                }
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }
    }

    fn validate_message(message: &NetworkMessage) -> bool {
        match message {
            NetworkMessage::NewTransaction(tx) => {
                tx.amount > 0.0
                    && tx.fee >= 0.0
                    && !tx.from.is_empty()
                    && !tx.to.is_empty()
                    && tx.id.len() == 64
            }
            NetworkMessage::NewBlock(block) => {
                !block.previous_hash.is_empty()
                    && block.hash.len() == 64
                    && block.difficulty <= 32
                    && !block.validator.is_empty()
            }
            // Non-state-changing messages can pass quickly
            _ => true,
        }
    }

    async fn submit_validated_message(
        message: NetworkMessage,
        blockchain: BlockchainHandle,
    ) -> Result<(), ()> {
        match message {
            NetworkMessage::NewBlock(block) => blockchain.add_block(block).await.map_err(|_| ()),
            NetworkMessage::NewTransaction(tx) => {
                blockchain.submit_transaction(tx).await.map_err(|_| ())
            }
            NetworkMessage::RequestChain => Ok(()),
            NetworkMessage::Ping => Ok(()),
            _ => Ok(()),
        }
    }

    pub async fn broadcast_block(&self, block: Block) -> Result<(), Box<dyn std::error::Error>> {
        let message = NetworkMessage::NewBlock(block);
        let serialized = serde_json::to_vec(&message)?;

        for peer in self.peers.values() {
            if let Ok(mut stream) = tokio::net::TcpStream::connect(&peer.address).await {
                let _ = stream.write_all(&serialized).await;
            }
        }

        Ok(())
    }

    pub async fn broadcast_transaction(
        &self,
        tx: Transaction,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let message = NetworkMessage::NewTransaction(tx);
        let serialized = serde_json::to_vec(&message)?;

        for peer in self.peers.values() {
            if let Ok(mut stream) = tokio::net::TcpStream::connect(&peer.address).await {
                let _ = stream.write_all(&serialized).await;
            }
        }

        Ok(())
    }

    pub fn add_peer(&mut self, address: String) {
        self.peers.insert(
            address.clone(),
            PeerInfo {
                address,
                last_seen: chrono::Utc::now().timestamp(),
                reputation: 1.0,
            },
        );
    }

    pub fn remove_peer(&mut self, address: &str) {
        self.peers.remove(address);
    }
}
