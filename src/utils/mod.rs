pub mod env_reader;
pub mod exif;
pub mod password_hash;
pub mod storage_resolver;

const BLAKE_3_LEN: usize = 32;

pub fn crop_blake_3_hash(hash: &[u8; BLAKE_3_LEN]) -> Vec<u8> {
    hash[..BLAKE_3_LEN / 2].to_vec()
}
