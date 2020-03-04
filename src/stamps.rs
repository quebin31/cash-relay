use bitcoin::consensus::encode::Error as TxDeserializeError;
use bitcoin::{util::psbt::serialize::Deserialize, Transaction};
use bitcoin_hashes::{hash160, sha256, Hash};
use secp256k1::{
    key::{PublicKey, SecretKey},
    Secp256k1,
};

use crate::bitcoin::{BitcoinClient, HttpConnector, NodeError};

#[derive(Debug)]
pub enum StampError {
    Decode(TxDeserializeError),
    MissingOutput,
    NotP2PKH,
    TxReject(NodeError),
    UnexpectedAddress,
    DegenerateCombination,
}

pub async fn verify_stamp(
    stamp_tx: &[u8],
    serialized_payload: &[u8],
    destination_pubkey: PublicKey,
    bitcoin_client: BitcoinClient<HttpConnector>,
) -> Result<(), StampError> {
    // Get pubkey hash from stamp tx
    let tx = Transaction::deserialize(stamp_tx).map_err(StampError::Decode)?;
    let output = tx.output.get(0).ok_or(StampError::MissingOutput)?;
    let script = &output.script_pubkey;
    if !script.is_p2pkh() {
        return Err(StampError::NotP2PKH);
    }
    let pubkey_hash = &script.as_bytes()[3..23]; // This is safe as we've checked it's a p2pkh

    // Calculate payload pubkey hash
    let payload_digest = sha256::Hash::hash(serialized_payload);
    let payload_secret_key = SecretKey::from_slice(&payload_digest).unwrap(); // TODO: Check this is safe
    let payload_public_key =
        PublicKey::from_secret_key(&Secp256k1::signing_only(), &payload_secret_key);

    // Combine keys
    let combined_key = destination_pubkey
        .combine(&payload_public_key)
        .map_err(|_| StampError::DegenerateCombination)?;
    let combine_key_raw = combined_key.serialize();
    let combine_pubkey_hash = hash160::Hash::hash(&combine_key_raw[..]).into_inner();

    // Check equivalence
    if combine_pubkey_hash != pubkey_hash {
        return Err(StampError::UnexpectedAddress);
    }

    bitcoin_client
        .send_tx(stamp_tx.to_vec())
        .await
        .map_err(StampError::TxReject)?;

    Ok(())
}
