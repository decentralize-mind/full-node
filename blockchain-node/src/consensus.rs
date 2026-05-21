use rand::Rng;

use super::block::Block;
use super::blockchain::Blockchain;

#[derive(Debug)]
pub struct Consensus {
    pub epoch_length: u64,
    pub min_stake: f64,
}

impl Consensus {
    pub fn new() -> Self {
        Consensus {
            epoch_length: 100,
            min_stake: 1.0,
        }
    }

    pub fn select_validator(&self, blockchain: &Blockchain) -> Option<String> {
        let total_stake: f64 = blockchain.validators.values().sum();

        if total_stake == 0.0 {
            return None;
        }

        let mut rng = rand::thread_rng();
        let random_value: f64 = rng.gen::<f64>() * total_stake;

        let mut cumulative_stake = 0.0;
        for (address, stake) in &blockchain.validators {
            cumulative_stake += stake;
            if random_value <= cumulative_stake {
                return Some(address.clone());
            }
        }

        None
    }

    pub fn validate_block(&self, block: &Block, blockchain: &Blockchain) -> bool {
        if !blockchain.validators.contains_key(&block.validator) {
            return false;
        }

        let stake = blockchain.validators.get(&block.validator).unwrap();
        if *stake < self.min_stake {
            return false;
        }

        true
    }

    pub fn slash_validator(&self, blockchain: &mut Blockchain, address: &str) {
        if let Some(stake) = blockchain.validators.get_mut(address) {
            *stake *= 0.5;
        }
    }
}
