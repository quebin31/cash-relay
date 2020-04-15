use std::fmt;

use bitcoincash_addr::Address;
use bytes::Bytes;
use prost::Message as _;
use rocksdb::Error as RocksError;
use secp256k1::{key::PublicKey, Error as SecpError, Message, Secp256k1, Signature};
use sha2::{Digest, Sha256};
use warp::{http::Response, hyper::Body, reject::Reject};

use super::IntoResponse;
use crate::{db::Database, models::wrapper::AuthWrapper};

#[derive(Debug)]
pub enum ProfileError {
    NotFound,
    Database(RocksError),
    InvalidSignature(SecpError),
    Message(SecpError),
    ProfileDecode(prost::DecodeError),
    PublicKey(SecpError),
    Signature(SecpError),
    UnsupportedScheme,
}

#[derive(Debug, Deserialize)]
pub struct Query {
    digest: Option<bool>,
}

impl From<RocksError> for ProfileError {
    fn from(err: RocksError) -> Self {
        ProfileError::Database(err)
    }
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let printable = match self {
            Self::NotFound => "not found",
            Self::Database(err) => return err.fmt(f),
            Self::InvalidSignature(err) => return err.fmt(f),
            Self::Message(err) => return err.fmt(f),
            Self::ProfileDecode(err) => return err.fmt(f),
            Self::PublicKey(err) => return err.fmt(f),
            Self::Signature(err) => return err.fmt(f),
            Self::UnsupportedScheme => "unsupported signature scheme",
        };
        f.write_str(printable)
    }
}

impl Reject for ProfileError {}

impl IntoResponse for ProfileError {
    fn to_status(&self) -> u16 {
        match self {
            Self::NotFound => 404,
            Self::Database(_) => 500,
            Self::UnsupportedScheme => 501,
            _ => 400,
        }
    }
}

pub async fn get_profile(
    addr: Address,
    query: Query,
    database: Database,
) -> Result<Response<Body>, ProfileError> {
    // Get profile
    let profile = database
        .get_profile(addr.as_body())?
        .ok_or(ProfileError::NotFound)?;

    // Serialize messages
    let mut raw_profile = Vec::with_capacity(profile.encoded_len());
    profile.encode(&mut raw_profile).unwrap();

    // Respond
    match query.digest {
        Some(true) => {
            let digest = Sha256::digest(&raw_profile).to_vec();
            Ok(Response::builder().body(Body::from(digest)).unwrap()) // TODO: Headers
        }
        _ => {
            Ok(Response::builder().body(Body::from(raw_profile)).unwrap()) // TODO: Headers
        }
    }
}

pub async fn put_profile(
    addr: Address,
    profile_raw: Bytes,
    db_data: Database,
) -> Result<Response<Body>, ProfileError> {
    // Decode profile
    let profile = AuthWrapper::decode(profile_raw.clone()).map_err(ProfileError::ProfileDecode)?;

    // Verify signatures
    let pubkey = PublicKey::from_slice(&profile.pub_key).map_err(ProfileError::PublicKey)?;
    if profile.scheme != 1 {
        // TODO: Support Schnorr
        return Err(ProfileError::UnsupportedScheme);
    }
    let signature = Signature::from_compact(&profile.signature).map_err(ProfileError::Signature)?;
    let secp = Secp256k1::verification_only();
    let payload_digest = Sha256::digest(&profile.serialized_payload);
    let msg = Message::from_slice(&payload_digest).map_err(ProfileError::Message)?;
    secp.verify(&msg, &signature, &pubkey)
        .map_err(ProfileError::InvalidSignature)?;

    // Put to database
    db_data.put_profile(addr.as_body(), &profile_raw)?;

    // Respond
    Ok(Response::builder().body(Body::empty()).unwrap())
}
