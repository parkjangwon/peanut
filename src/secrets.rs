use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use openssl::{sha::sha256, symm::{Cipher, Crypter, Mode}};
use rand::RngCore;

const SECRET_CIPHERTEXT_PREFIX: &str = "v1";
const SECRET_ENCRYPTION_VERSION: i64 = 1;
const SECRET_NONCE_BYTES: usize = 12;
const SECRET_TAG_BYTES: usize = 16;

pub fn encryption_version() -> i64 {
    SECRET_ENCRYPTION_VERSION
}

pub fn encrypt_secret(master_key: &str, plaintext: &str) -> Result<String, String> {
    let key = derive_key(master_key);
    let mut nonce = [0u8; SECRET_NONCE_BYTES];
    rand::thread_rng().fill_bytes(&mut nonce);

    let cipher = Cipher::aes_256_gcm();
    let mut crypter =
        Crypter::new(cipher, Mode::Encrypt, &key, Some(&nonce)).map_err(|_| "failed to initialize secret encryption".to_string())?;
    crypter.pad(false);

    let mut ciphertext = vec![0u8; plaintext.len() + cipher.block_size()];
    let mut count = crypter
        .update(plaintext.as_bytes(), &mut ciphertext)
        .map_err(|_| "failed to encrypt secret".to_string())?;
    count += crypter
        .finalize(&mut ciphertext[count..])
        .map_err(|_| "failed to finalize secret encryption".to_string())?;
    ciphertext.truncate(count);

    let mut tag = [0u8; SECRET_TAG_BYTES];
    crypter
        .get_tag(&mut tag)
        .map_err(|_| "failed to capture secret encryption tag".to_string())?;

    Ok(format!(
        "{}:{}:{}:{}",
        SECRET_CIPHERTEXT_PREFIX,
        URL_SAFE_NO_PAD.encode(nonce),
        URL_SAFE_NO_PAD.encode(ciphertext),
        URL_SAFE_NO_PAD.encode(tag)
    ))
}

pub fn decrypt_secret(master_key: &str, ciphertext: &str) -> Result<String, String> {
    let mut parts = ciphertext.split(':');
    let version = parts.next().ok_or_else(|| "encrypted secret is missing version".to_string())?;
    if version != SECRET_CIPHERTEXT_PREFIX {
        return Err("encrypted secret uses an unsupported version".to_string());
    }

    let nonce = decode_component(parts.next(), "nonce")?;
    let ciphertext_bytes = decode_component(parts.next(), "ciphertext")?;
    let tag = decode_component(parts.next(), "tag")?;
    if parts.next().is_some() {
        return Err("encrypted secret has an invalid format".to_string());
    }

    let key = derive_key(master_key);
    let cipher = Cipher::aes_256_gcm();
    let mut crypter =
        Crypter::new(cipher, Mode::Decrypt, &key, Some(&nonce)).map_err(|_| "failed to initialize secret decryption".to_string())?;
    crypter.pad(false);
    crypter
        .set_tag(&tag)
        .map_err(|_| "failed to apply secret decryption tag".to_string())?;

    let mut plaintext = vec![0u8; ciphertext_bytes.len() + cipher.block_size()];
    let mut count = crypter
        .update(&ciphertext_bytes, &mut plaintext)
        .map_err(|_| "failed to decrypt secret".to_string())?;
    count += crypter
        .finalize(&mut plaintext[count..])
        .map_err(|_| "failed to finalize secret decryption".to_string())?;
    plaintext.truncate(count);

    String::from_utf8(plaintext).map_err(|_| "decrypted secret is not valid UTF-8".to_string())
}

fn derive_key(master_key: &str) -> [u8; 32] {
    sha256(master_key.as_bytes())
}

fn decode_component(value: Option<&str>, label: &str) -> Result<Vec<u8>, String> {
    let raw = value.ok_or_else(|| format!("encrypted secret is missing {label}"))?;
    URL_SAFE_NO_PAD
        .decode(raw)
        .map_err(|_| format!("encrypted secret has an invalid {label}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_encryption_round_trip() {
        let ciphertext = encrypt_secret("master-key", "super-secret").unwrap();
        assert_ne!(ciphertext, "super-secret");
        assert_eq!(decrypt_secret("master-key", &ciphertext).unwrap(), "super-secret");
    }

    #[test]
    fn test_secret_decryption_rejects_wrong_key() {
        let ciphertext = encrypt_secret("master-key", "super-secret").unwrap();
        assert!(decrypt_secret("other-key", &ciphertext).is_err());
    }
}
