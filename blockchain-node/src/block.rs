use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::transaction::Transaction;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub index: u64,
    pub timestamp: i64,
    pub transactions: Vec<Transaction>,
    pub previous_hash: String,
    pub hash: String,
    pub nonce: u64,
    pub difficulty: u32,
    pub merkle_root: String,
    pub validator: String,
}

impl Block {
    pub fn new(
        index: u64,
        transactions: Vec<Transaction>,
        previous_hash: String,
        difficulty: u32,
        validator: String,
    ) -> Self {
        let timestamp = Utc::now().timestamp();
        let merkle_root = Self::calculate_merkle_root(&transactions);

        let mut block = Block {
            index,
            timestamp,
            transactions,
            previous_hash,
            hash: String::new(),
            nonce: 0,
            difficulty,
            merkle_root,
            validator,
        };

        block.hash = block.calculate_hash();
        block
    }

    pub fn calculate_hash(&self) -> String {
        let mut hasher = Sha256::new();
        let data = format!(
            "{}{}{}{}{}{}{}",
            self.index,
            self.timestamp,
            self.merkle_root,
            self.previous_hash,
            self.nonce,
            self.difficulty,
            self.validator
        );
        hasher.update(data.as_bytes());
        hex::encode(hasher.finalize())
    }

    pub fn calculate_merkle_root(transactions: &[Transaction]) -> String {
        if transactions.is_empty() {
            return String::from(
                "0000000000000000000000000000000000000000000000000000000000000000",
            );
        }

        let mut hashes: Vec<String> = transactions.iter().map(|tx| tx.calculate_hash()).collect();

        while hashes.len() > 1 {
            let mut new_hashes = Vec::new();
            for chunk in hashes.chunks(2) {
                let mut hasher = Sha256::new();
                if chunk.len() == 2 {
                    hasher.update(format!("{}{}", chunk[0], chunk[1]).as_bytes());
                } else {
                    hasher.update(format!("{}{}", chunk[0], chunk[0]).as_bytes());
                }
                new_hashes.push(hex::encode(hasher.finalize()));
            }
            hashes = new_hashes;
        }

        hashes[0].clone()
    }

    pub fn mine_block(&mut self) {
        let target = "0".repeat(self.difficulty as usize);
        while &self.hash[..self.difficulty as usize] != target {
            self.nonce += 1;
            self.hash = self.calculate_hash();
        }
    }

    pub fn is_valid(&self) -> bool {
        if self.hash != self.calculate_hash() {
            return false;
        }

        let target = "0".repeat(self.difficulty as usize);
        if &self.hash[..self.difficulty as usize] != target {
            return false;
        }

        for tx in &self.transactions {
            if !tx.is_valid() {
                return false;
            }
        }

        if self.merkle_root != Self::calculate_merkle_root(&self.transactions) {
            return false;
        }

        true
    }
}
