use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::crypto::Signature;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub id: String,
    pub from: String,
    pub to: String,
    pub amount: f64,
    pub fee: f64,
    pub timestamp: i64,
    pub signature: Option<Signature>,
    pub data: Option<String>,
}

impl Transaction {
    pub fn new(from: String, to: String, amount: f64, fee: f64) -> Self {
        let timestamp = Utc::now().timestamp();
        let mut tx = Transaction {
            id: String::new(),
            from,
            to,
            amount,
            fee,
            timestamp,
            signature: None,
            data: None,
        };
        tx.id = tx.calculate_hash();
        tx
    }

    pub fn calculate_hash(&self) -> String {
        let mut hasher = Sha256::new();
        let data = format!(
            "{}{}{}{}{}{:?}",
            self.from, self.to, self.amount, self.fee, self.timestamp, self.data
        );
        hasher.update(data.as_bytes());
        hex::encode(hasher.finalize())
    }

    pub fn sign(&mut self, signature: Signature) {
        self.signature = Some(signature);
    }

    pub fn verify(&self) -> bool {
        if let Some(sig) = &self.signature {
            let message = self.calculate_hash();
            sig.verify(&message, &self.from)
        } else {
            false
        }
    }

    pub fn is_valid(&self) -> bool {
        self.amount > 0.0
            && self.fee >= 0.0
            && !self.from.is_empty()
            && !self.to.is_empty()
            && self.verify()
    }
}
