pub mod actor;
pub mod block;
pub mod blockchain;
pub mod consensus;
pub mod crypto;
pub mod mempool;
pub mod network;
pub mod state;
pub mod transaction;
pub mod wallet;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum BlockchainError {
    #[error("Invalid block")]
    InvalidBlock,
    #[error("Invalid transaction")]
    InvalidTransaction,
    #[error("Chain validation failed")]
    ChainValidationFailed,
    #[error("Network error: {0}")]
    NetworkError(String),
    #[error("Consensus error: {0}")]
    ConsensusError(String),
}
