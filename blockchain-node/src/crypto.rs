use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signature {
    pub r: String,
    pub s: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyPair {
    pub private_key: String,
    pub public_key: String,
}

impl KeyPair {
    pub fn generate() -> Self {
        let mut rng = OsRng;
        let private_key = hex::encode(rand::RngCore::next_u64(&mut rng).to_le_bytes());
        let public_key = Self::derive_public_key(&private_key);

        KeyPair {
            private_key,
            public_key,
        }
    }

    fn derive_public_key(private_key: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(private_key.as_bytes());
        hex::encode(hasher.finalize())
    }
}

impl Signature {
    pub fn sign(message: &str, private_key: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(format!("{}{}", message, private_key).as_bytes());
        let hash = hasher.finalize();

        Signature {
            r: hex::encode(&hash[..16]),
            s: hex::encode(&hash[16..]),
        }
    }

    pub fn verify(&self, message: &str, public_key: &str) -> bool {
        let mut hasher = Sha256::new();
        hasher.update(format!("{}{}", message, public_key).as_bytes());
        let expected_hash = hasher.finalize();

        let r_bytes = hex::decode(&self.r).unwrap_or_default();
        let s_bytes = hex::decode(&self.s).unwrap_or_default();

        expected_hash[..16] == r_bytes[..] && expected_hash[16..] == s_bytes[..]
    }
}
