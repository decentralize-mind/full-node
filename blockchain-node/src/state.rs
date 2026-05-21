use std::collections::HashMap;

use super::transaction::Transaction;
use super::BlockchainError;

#[derive(Debug)]
pub struct StateManager {
    pub balances: HashMap<String, f64>,
    pub nonces: HashMap<String, u64>,
    pub contracts: HashMap<String, Vec<u8>>,
}

impl StateManager {
    pub fn new() -> Self {
        StateManager {
            balances: HashMap::new(),
            nonces: HashMap::new(),
            contracts: HashMap::new(),
        }
    }

    pub fn get_balance(&self, address: &str) -> f64 {
        *self.balances.get(address).unwrap_or(&0.0)
    }

    pub fn get_nonce(&self, address: &str) -> u64 {
        *self.nonces.get(address).unwrap_or(&0)
    }

    pub fn execute_transaction(&mut self, tx: &Transaction) -> Result<(), BlockchainError> {
        let sender_balance = self.get_balance(&tx.from);
        if tx.from != "genesis" && tx.from != "network" && sender_balance < tx.amount + tx.fee {
            return Err(BlockchainError::InvalidTransaction);
        }

        if tx.from != "genesis" && tx.from != "network" {
            self.balances
                .insert(tx.from.clone(), sender_balance - tx.amount - tx.fee);
        }

        let receiver_balance = self.get_balance(&tx.to);
        self.balances
            .insert(tx.to.clone(), receiver_balance + tx.amount);

        let nonce = self.get_nonce(&tx.from);
        self.nonces.insert(tx.from.clone(), nonce + 1);

        Ok(())
    }

    pub fn deploy_contract(&mut self, address: String, bytecode: Vec<u8>) {
        self.contracts.insert(address, bytecode);
    }

    pub fn get_contract(&self, address: &str) -> Option<&Vec<u8>> {
        self.contracts.get(address)
    }
}
