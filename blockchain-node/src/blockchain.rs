use std::collections::HashMap;

use super::block::Block;
use super::mempool::Mempool;
use super::state::StateManager;
use super::transaction::Transaction;
use super::BlockchainError;

#[derive(Debug)]
pub struct Blockchain {
    pub chain: Vec<Block>,
    pub pending_transactions: Vec<Transaction>,
    pub difficulty: u32,
    pub mining_reward: f64,
    pub state: StateManager,
    pub mempool: Mempool,
    pub validators: HashMap<String, f64>,
}

impl Blockchain {
    pub fn new() -> Self {
        let mut blockchain = Blockchain {
            chain: Vec::new(),
            pending_transactions: Vec::new(),
            difficulty: 4,
            mining_reward: 50.0,
            state: StateManager::new(),
            mempool: Mempool::new(),
            validators: HashMap::new(),
        };

        blockchain.create_genesis_block();
        blockchain
    }

    fn create_genesis_block(&mut self) {
        let genesis_block = Block::new(
            0,
            vec![],
            String::from("0"),
            self.difficulty,
            String::from("genesis"),
        );
        self.chain.push(genesis_block);
    }

    pub fn get_latest_block(&self) -> &Block {
        self.chain.last().unwrap()
    }

    pub fn add_block(&mut self, block: Block) -> Result<(), BlockchainError> {
        let latest_block = self.get_latest_block();
        if block.previous_hash != latest_block.hash {
            return Err(BlockchainError::InvalidBlock);
        }

        for tx in &block.transactions {
            self.state.execute_transaction(tx)?;
        }

        for tx in &block.transactions {
            self.mempool.remove_transaction(&tx.id);
        }

        if !block.transactions.is_empty() {
            let reward_tx = Transaction::new(
                String::from("network"),
                block.validator.clone(),
                self.mining_reward,
                0.0,
            );
            self.state.execute_transaction(&reward_tx)?;
        }

        self.chain.push(block);
        Ok(())
    }

    pub fn add_transaction(&mut self, transaction: Transaction) -> Result<(), BlockchainError> {
        if !self.validate_transaction(&transaction) {
            return Err(BlockchainError::InvalidTransaction);
        }

        self.mempool.add_transaction(transaction.clone());
        self.pending_transactions.push(transaction);
        Ok(())
    }

    pub fn validate_transaction(&self, transaction: &Transaction) -> bool {
        let sender_balance = self.state.get_balance(&transaction.from);
        sender_balance >= transaction.amount + transaction.fee
    }

    pub fn is_chain_valid(&self) -> bool {
        for i in 1..self.chain.len() {
            let current_block = &self.chain[i];
            let previous_block = &self.chain[i - 1];

            if current_block.previous_hash != previous_block.hash {
                return false;
            }
        }
        true
    }

    pub fn get_balance(&self, address: &str) -> f64 {
        self.state.get_balance(address)
    }

    pub fn stake_tokens(&mut self, address: String, amount: f64) -> Result<(), BlockchainError> {
        *self.validators.entry(address).or_insert(0.0) += amount;
        Ok(())
    }
}
