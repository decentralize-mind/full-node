use std::collections::HashMap;

use super::transaction::Transaction;

#[derive(Debug)]
pub struct Mempool {
    pub transactions: HashMap<String, Transaction>,
    pub max_size: usize,
    pub min_fee: f64,
}

impl Mempool {
    pub fn new() -> Self {
        Mempool {
            transactions: HashMap::new(),
            max_size: 1000,
            min_fee: 0.001,
        }
    }

    pub fn add_transaction(&mut self, tx: Transaction) {
        if self.transactions.len() >= self.max_size {
            self.remove_lowest_fee();
        }
        self.transactions.insert(tx.id.clone(), tx);
    }

    pub fn remove_transaction(&mut self, tx_id: &str) {
        self.transactions.remove(tx_id);
    }

    pub fn get_transactions(&self, limit: usize) -> Vec<Transaction> {
        let mut txs: Vec<Transaction> = self.transactions.values().cloned().collect();
        txs.sort_by(|a, b| b.fee.partial_cmp(&a.fee).unwrap());
        txs.truncate(limit);
        txs
    }

    fn remove_lowest_fee(&mut self) {
        if let Some(lowest_fee_tx) = self
            .transactions
            .values()
            .min_by(|a, b| a.fee.partial_cmp(&b.fee).unwrap())
            .map(|tx| tx.id.clone())
        {
            self.transactions.remove(&lowest_fee_tx);
        }
    }

    pub fn size(&self) -> usize {
        self.transactions.len()
    }
}
