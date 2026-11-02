use anyhow::{Context, Result, bail};
use ring::rand::SystemRandom;
use ring::signature::{self, Ed25519KeyPair, KeyPair, UnparsedPublicKey};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const SIGNATURE_SCHEMA: u32 = 1;
pub const SIGNATURE_ALGORITHM: &str = "ed25519";
pub const SIGNATURE_SUFFIX: &str = ".sig";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SignatureFile {
    pub schema: u32,
    pub algorithm: String,
    pub public_key: String,
    pub signature: String,
}

#[derive(Clone, Debug, Default)]
pub struct Trust {
    pub keys: Vec<String>,
    pub allow_unsigned: bool,
}

impl Trust {
    pub fn new(keys: &[String], allow_unsigned: bool) -> Self {
        Self {
            keys: keys
                .iter()
                .map(|key| key.trim().to_ascii_lowercase())
                .filter(|key| !key.is_empty())
                .collect(),
            allow_unsigned,
        }
    }

    pub fn is_trusted(&self, key: &str) -> bool {
        let key = key.trim().to_ascii_lowercase();
        self.keys.iter().any(|candidate| candidate == &key)
    }
}

pub fn signature_path(index: &Path) -> PathBuf {
    let mut name = index.as_os_str().to_os_string();
    name.push(SIGNATURE_SUFFIX);
    PathBuf::from(name)
}

pub fn public_key_hex(pair: &Ed25519KeyPair) -> String {
    hex::encode(pair.public_key().as_ref())
}

pub fn keygen(directory: &Path) -> Result<(PathBuf, PathBuf)> {
    fs::create_dir_all(directory).with_context(|| format!("creating {}", directory.display()))?;
    let rng = SystemRandom::new();
    let document = Ed25519KeyPair::generate_pkcs8(&rng)
        .map_err(|_| anyhow::anyhow!("generating an Ed25519 key pair"))?;
    let pair = Ed25519KeyPair::from_pkcs8(document.as_ref())
        .map_err(|_| anyhow::anyhow!("loading the generated key pair"))?;
    let private = directory.join("rivet-signing.key");
    let public = directory.join("rivet-signing.pub");
    fs::write(&private, document.as_ref())
        .with_context(|| format!("writing {}", private.display()))?;
    fs::write(&public, format!("{}\n", public_key_hex(&pair)))
        .with_context(|| format!("writing {}", public.display()))?;
    Ok((private, public))
}

pub fn load_private_key(path: &Path) -> Result<Ed25519KeyPair> {
    let bytes =
        fs::read(path).with_context(|| format!("reading signing key {}", path.display()))?;
    Ed25519KeyPair::from_pkcs8(&bytes)
        .map_err(|_| anyhow::anyhow!("{} is not a valid Ed25519 private key", path.display()))
}

pub fn sign(pair: &Ed25519KeyPair, data: &[u8]) -> SignatureFile {
    let signature = pair.sign(data);
    SignatureFile {
        schema: SIGNATURE_SCHEMA,
        algorithm: SIGNATURE_ALGORITHM.to_string(),
        public_key: public_key_hex(pair),
        signature: hex::encode(signature.as_ref()),
    }
}

pub fn verify_signature(
    data: &[u8],
    signature: &SignatureFile,
    trust: &Trust,
    location: &str,
) -> Result<()> {
    if signature.schema != SIGNATURE_SCHEMA {
        bail!(
            "unsupported index signature schema {} for {location}",
            signature.schema
        );
    }
    if !signature
        .algorithm
        .eq_ignore_ascii_case(SIGNATURE_ALGORITHM)
    {
        bail!(
            "unsupported index signature algorithm '{}' for {location}",
            signature.algorithm
        );
    }
    if trust.keys.is_empty() {
        bail!(
            "no trusted signing key is configured; refusing the index at {location}.\n  Trust the maintainer key with 'rv config set trusted_key <hex>' or set RIVET_TRUSTED_KEY."
        );
    }
    if !trust.is_trusted(&signature.public_key) {
        bail!(
            "index {location} is signed by an untrusted key {}.\n  Trust it with 'rv config set trusted_key {}' only if you expect that key.",
            signature.public_key,
            signature.public_key
        );
    }
    let key = hex::decode(signature.public_key.trim())
        .context("index signature has a malformed public key")?;
    let raw = hex::decode(signature.signature.trim())
        .context("index signature has a malformed signature")?;
    UnparsedPublicKey::new(&signature::ED25519, key)
        .verify(data, &raw)
        .map_err(|_| anyhow::anyhow!("index signature verification failed for {location}"))
}

pub fn verify_index(
    data: &[u8],
    signature: Option<&str>,
    trust: &Trust,
    location: &str,
    remote: bool,
) -> Result<()> {
    match signature {
        Some(text) => {
            let parsed: SignatureFile = serde_json::from_str(text)
                .with_context(|| format!("parsing the index signature for {location}"))?;
            verify_signature(data, &parsed, trust, location)
        }
        None if remote && !trust.allow_unsigned => bail!(
            "refusing unsigned remote index {location}: {location}{SIGNATURE_SUFFIX} was not found.\n  Sign the index with 'rv repo sign' and trust the key with 'rv config set trusted_key <hex>'.\n  Pass --insecure to override (not recommended)."
        ),
        None => Ok(()),
    }
}
