use serde::{Deserialize, Serialize};

use super::crypto::KeyPair;
use super::transaction::Transaction;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wallet {
    pub address: String,
    pub keypair: KeyPair,
    pub balance: f64,
}

impl Wallet {
    pub fn new() -> Self {
        let keypair = KeyPair::generate();
        let address = keypair.public_key.clone();

        Wallet {
            address,
            keypair,
            balance: 0.0,
        }
    }

    pub fn create_transaction(
        &self,
        to: String,
        amount: f64,
        fee: f64,
    ) -> Result<Transaction, String> {
        if self.balance < amount + fee {
            return Err("Insufficient balance".to_string());
        }

        let mut tx = Transaction::new(self.address.clone(), to, amount, fee);

        let signature =
            super::crypto::Signature::sign(&tx.calculate_hash(), &self.keypair.private_key);
        tx.sign(signature);

        Ok(tx)
    }

    pub fn get_public_key(&self) -> String {
        self.keypair.public_key.clone()
    }
}
