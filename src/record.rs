use anyhow::{anyhow, Result};

const MAX_KEY_LEN: usize = 1024 * 1024;      // 1MB
const MAX_VAL_LEN: usize = 1024 * 1024 * 10; // 10MB
const CRC_SIZE: usize = 4;

#[derive(Debug, Clone)]
pub struct Record {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
}

impl Record {
    pub fn new(key: Vec<u8>, value: Vec<u8>) -> Self {
        Self { key, value }
    }

    pub fn encode(&self) -> Vec<u8> {
        let key_len = self.key.len() as u32;
        let val_len = self.value.len() as u32;
        let mut buf = Vec::with_capacity(8 + self.key.len() + self.value.len() + CRC_SIZE);
        
        buf.extend_from_slice(&key_len.to_le_bytes());
        buf.extend_from_slice(&val_len.to_le_bytes());
        buf.extend_from_slice(&self.key);
        buf.extend_from_slice(&self.value);
        
        let crc = crc32fast::hash(&buf);
        buf.extend_from_slice(&crc.to_le_bytes());
        buf
    }

    pub fn decode(buf: &[u8]) -> Result<(Self, usize)> {
        if buf.len() < 12 { return Err(anyhow!("Buffer too short")); }

        let key_len = u32::from_le_bytes(buf[0..4].try_into().unwrap()) as usize;
        let val_len = u32::from_le_bytes(buf[4..8].try_into().unwrap()) as usize;
        let total_len = 8 + key_len + val_len + CRC_SIZE;

        if buf.len() < total_len { return Err(anyhow!("Incomplete buffer")); }

        let data_end = total_len - CRC_SIZE;
        let expected_crc = u32::from_le_bytes(buf[data_end..total_len].try_into().unwrap());
        let actual_crc = crc32fast::hash(&buf[..data_end]);
        
        if expected_crc != actual_crc {
            return Err(anyhow!("CRC mismatch"));
        }

        let key = buf[8..8 + key_len].to_vec();
        let value = buf[8 + key_len..8 + key_len + val_len].to_vec();
        Ok((Record { key, value }, total_len))
    }
}