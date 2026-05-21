use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::{mpsc, oneshot};

use crate::block::Block;
use crate::blockchain::Blockchain;
use crate::consensus::Consensus;
use crate::transaction::Transaction;
use crate::BlockchainError;

#[derive(Clone, Debug)]
pub struct BlockchainHandle {
    sender: mpsc::Sender<BlockchainEnvelope>,
    metrics: ActorMetricsHandle,
}

#[derive(Debug)]
struct BlockchainEnvelope {
    command: BlockchainCommand,
    enqueued_at: Instant,
}

#[derive(Debug)]
enum BlockchainCommand {
    BootstrapBalance {
        address: String,
        amount: f64,
        reply: oneshot::Sender<Result<(), BlockchainError>>,
    },
    SubmitTransactionUnchecked {
        tx: Transaction,
        reply: oneshot::Sender<()>,
    },
    SubmitTransaction {
        tx: Transaction,
        reply: oneshot::Sender<Result<(), BlockchainError>>,
    },
    AddBlock {
        block: Block,
        reply: oneshot::Sender<Result<(), BlockchainError>>,
    },
    StakeTokens {
        address: String,
        amount: f64,
        reply: oneshot::Sender<Result<(), BlockchainError>>,
    },
    MinePendingBlock {
        limit: usize,
        reply: oneshot::Sender<Result<Option<Block>, BlockchainError>>,
    },
    GetBalance {
        address: String,
        reply: oneshot::Sender<f64>,
    },
    GetChainLength {
        reply: oneshot::Sender<usize>,
    },
    IsChainValid {
        reply: oneshot::Sender<bool>,
    },
}

#[derive(Clone, Debug)]
pub struct ActorMetricsHandle {
    inner: Arc<ActorMetricsInner>,
}

#[derive(Debug)]
struct ActorMetricsInner {
    queue_depth: AtomicUsize,
    processed_total: AtomicU64,
    total_latency_ns: AtomicU64,
    latency_samples: AtomicU64,
}

#[derive(Clone, Debug)]
pub struct ActorMetricsSnapshot {
    pub queue_depth: usize,
    pub processed_total: u64,
    pub avg_latency_ms: f64,
}

impl ActorMetricsHandle {
    fn new() -> Self {
        Self {
            inner: Arc::new(ActorMetricsInner {
                queue_depth: AtomicUsize::new(0),
                processed_total: AtomicU64::new(0),
                total_latency_ns: AtomicU64::new(0),
                latency_samples: AtomicU64::new(0),
            }),
        }
    }

    fn on_enqueued(&self) {
        self.inner.queue_depth.fetch_add(1, Ordering::Relaxed);
    }

    fn on_dequeued(&self, elapsed: std::time::Duration) {
        self.inner.queue_depth.fetch_sub(1, Ordering::Relaxed);
        self.inner.processed_total.fetch_add(1, Ordering::Relaxed);
        self.inner
            .total_latency_ns
            .fetch_add(elapsed.as_nanos() as u64, Ordering::Relaxed);
        self.inner.latency_samples.fetch_add(1, Ordering::Relaxed);
    }

    fn on_enqueue_failed(&self) {
        self.inner.queue_depth.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> ActorMetricsSnapshot {
        let queue_depth = self.inner.queue_depth.load(Ordering::Relaxed);
        let processed_total = self.inner.processed_total.load(Ordering::Relaxed);
        let samples = self.inner.latency_samples.load(Ordering::Relaxed);
        let total_latency_ns = self.inner.total_latency_ns.load(Ordering::Relaxed);
        let avg_latency_ms = if samples == 0 {
            0.0
        } else {
            (total_latency_ns as f64 / samples as f64) / 1_000_000.0
        };

        ActorMetricsSnapshot {
            queue_depth,
            processed_total,
            avg_latency_ms,
        }
    }
}

impl BlockchainHandle {
    pub fn start(buffer: usize) -> Self {
        let (tx, mut rx) = mpsc::channel::<BlockchainEnvelope>(buffer);
        let metrics = ActorMetricsHandle::new();
        let actor_metrics = metrics.clone();

        tokio::spawn(async move {
            let mut blockchain = Blockchain::new();
            let consensus = Consensus::new();

            while let Some(envelope) = rx.recv().await {
                actor_metrics.on_dequeued(envelope.enqueued_at.elapsed());

                match envelope.command {
                    BlockchainCommand::BootstrapBalance {
                        address,
                        amount,
                        reply,
                    } => {
                        let tx = Transaction::new("genesis".to_string(), address, amount, 0.0);
                        let _ = reply.send(blockchain.state.execute_transaction(&tx));
                    }
                    BlockchainCommand::SubmitTransactionUnchecked { tx, reply } => {
                        blockchain.mempool.add_transaction(tx.clone());
                        blockchain.pending_transactions.push(tx);
                        let _ = reply.send(());
                    }
                    BlockchainCommand::SubmitTransaction { tx, reply } => {
                        let _ = reply.send(blockchain.add_transaction(tx));
                    }
                    BlockchainCommand::AddBlock { block, reply } => {
                        let _ = reply.send(blockchain.add_block(block));
                    }
                    BlockchainCommand::StakeTokens {
                        address,
                        amount,
                        reply,
                    } => {
                        let _ = reply.send(blockchain.stake_tokens(address, amount));
                    }
                    BlockchainCommand::MinePendingBlock { limit, reply } => {
                        let result =
                            if let Some(validator) = consensus.select_validator(&blockchain) {
                                let transactions = blockchain.mempool.get_transactions(limit);
                                if transactions.is_empty() {
                                    Ok(None)
                                } else {
                                    let previous_hash = blockchain.get_latest_block().hash.clone();
                                    let mut block = Block::new(
                                        blockchain.chain.len() as u64,
                                        transactions,
                                        previous_hash,
                                        blockchain.difficulty,
                                        validator,
                                    );
                                    block.mine_block();
                                    match blockchain.add_block(block.clone()) {
                                        Ok(()) => Ok(Some(block)),
                                        Err(err) => Err(err),
                                    }
                                }
                            } else {
                                Ok(None)
                            };

                        let _ = reply.send(result);
                    }
                    BlockchainCommand::GetBalance { address, reply } => {
                        let _ = reply.send(blockchain.get_balance(&address));
                    }
                    BlockchainCommand::GetChainLength { reply } => {
                        let _ = reply.send(blockchain.chain.len());
                    }
                    BlockchainCommand::IsChainValid { reply } => {
                        let _ = reply.send(blockchain.is_chain_valid());
                    }
                }
            }
        });

        Self {
            sender: tx,
            metrics,
        }
    }

    pub fn metrics(&self) -> ActorMetricsHandle {
        self.metrics.clone()
    }

    async fn send_enveloped(&self, command: BlockchainCommand) -> Result<(), BlockchainError> {
        self.metrics.on_enqueued();
        let envelope = BlockchainEnvelope {
            command,
            enqueued_at: Instant::now(),
        };

        if self.sender.send(envelope).await.is_err() {
            self.metrics.on_enqueue_failed();
            return Err(BlockchainError::NetworkError(
                "blockchain actor unavailable".into(),
            ));
        }

        Ok(())
    }

    pub async fn bootstrap_balance(
        &self,
        address: String,
        amount: f64,
    ) -> Result<(), BlockchainError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send_enveloped(BlockchainCommand::BootstrapBalance {
            address,
            amount,
            reply: reply_tx,
        })
        .await?;

        reply_rx
            .await
            .map_err(|_| BlockchainError::NetworkError("actor reply dropped".into()))?
    }

    pub async fn submit_transaction_unchecked(
        &self,
        tx: Transaction,
    ) -> Result<(), BlockchainError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send_enveloped(BlockchainCommand::SubmitTransactionUnchecked {
            tx,
            reply: reply_tx,
        })
        .await?;

        reply_rx
            .await
            .map_err(|_| BlockchainError::NetworkError("actor reply dropped".into()))?;

        Ok(())
    }

    pub async fn submit_transaction(&self, tx: Transaction) -> Result<(), BlockchainError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send_enveloped(BlockchainCommand::SubmitTransaction {
            tx,
            reply: reply_tx,
        })
        .await?;

        reply_rx
            .await
            .map_err(|_| BlockchainError::NetworkError("actor reply dropped".into()))?
    }

    pub async fn add_block(&self, block: Block) -> Result<(), BlockchainError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send_enveloped(BlockchainCommand::AddBlock {
            block,
            reply: reply_tx,
        })
        .await?;

        reply_rx
            .await
            .map_err(|_| BlockchainError::NetworkError("actor reply dropped".into()))?
    }

    pub async fn stake_tokens(&self, address: String, amount: f64) -> Result<(), BlockchainError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send_enveloped(BlockchainCommand::StakeTokens {
            address,
            amount,
            reply: reply_tx,
        })
        .await?;

        reply_rx
            .await
            .map_err(|_| BlockchainError::NetworkError("actor reply dropped".into()))?
    }

    pub async fn mine_pending_block(&self, limit: usize) -> Result<Option<Block>, BlockchainError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send_enveloped(BlockchainCommand::MinePendingBlock {
            limit,
            reply: reply_tx,
        })
        .await?;

        reply_rx
            .await
            .map_err(|_| BlockchainError::NetworkError("actor reply dropped".into()))?
    }

    pub async fn get_balance(&self, address: String) -> Result<f64, BlockchainError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send_enveloped(BlockchainCommand::GetBalance {
            address,
            reply: reply_tx,
        })
        .await?;

        reply_rx
            .await
            .map_err(|_| BlockchainError::NetworkError("actor reply dropped".into()))
    }

    pub async fn get_chain_length(&self) -> Result<usize, BlockchainError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send_enveloped(BlockchainCommand::GetChainLength { reply: reply_tx })
            .await?;

        reply_rx
            .await
            .map_err(|_| BlockchainError::NetworkError("actor reply dropped".into()))
    }

    pub async fn is_chain_valid(&self) -> Result<bool, BlockchainError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send_enveloped(BlockchainCommand::IsChainValid { reply: reply_tx })
            .await?;

        reply_rx
            .await
            .map_err(|_| BlockchainError::NetworkError("actor reply dropped".into()))
    }
}
