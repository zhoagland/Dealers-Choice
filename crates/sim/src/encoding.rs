//! Compact binary hand records (`docs/tech/sim.md` §4).
//!
//! A record holds what's needed for exact replay: versions, setup, and the action log, chance
//! included. Hidden cards are *in* the log (as chance actions), so nothing is stored twice.
//! Encoding: `postcard`, a compact serde format using varints, so small numbers take one byte.
//! Framing: a 4-byte little-endian length before each record, so a file of records can be read
//! back sequentially.

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use engine::{API_VERSION, HandSetup};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HandRecord<A> {
    pub api_version: u32,
    pub game_id: String,
    pub game_version: u32,
    /// Deal index within the run, and seat rotation (duplicate mode).
    pub deal: u64,
    pub rotation: u8,
    /// Which bot (index into the run's bot list) sat in each seat.
    pub seating: Vec<u8>,
    pub setup: HandSetup,
    /// API v2: the game's carry at the start of this hand (`postcard` bytes of `Game::Carry`,
    /// empty for games without one), so every hand of an `UntilDone` game replays on its own.
    pub carry_in: Vec<u8>,
    pub actions: Vec<A>,
}

impl<A> HandRecord<A> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        game_id: &str,
        game_version: u32,
        deal: u64,
        rotation: u8,
        seating: Vec<u8>,
        setup: HandSetup,
        carry_in: Vec<u8>,
        actions: Vec<A>,
    ) -> Self {
        HandRecord {
            api_version: API_VERSION,
            game_id: game_id.into(),
            game_version,
            deal,
            rotation,
            seating,
            setup,
            carry_in,
            actions,
        }
    }
}

/// Encode a carry for [`HandRecord::carry_in`].
pub fn encode_carry<C: Serialize>(carry: &C) -> Vec<u8> {
    postcard::to_allocvec(carry).expect("carries always serialize")
}

/// Decode [`HandRecord::carry_in`].
pub fn decode_carry<C: DeserializeOwned>(bytes: &[u8]) -> Result<C, String> {
    postcard::from_bytes(bytes).map_err(|e| e.to_string())
}

/// Encode with a length prefix.
pub fn encode<A: Serialize>(r: &HandRecord<A>) -> Vec<u8> {
    let body = postcard::to_allocvec(r).expect("hand records always serialize");
    let mut out = Vec::with_capacity(body.len() + 4);
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out
}

/// Decode every record in a buffer of length-prefixed records.
pub fn decode_all<A: DeserializeOwned>(mut bytes: &[u8]) -> Result<Vec<HandRecord<A>>, String> {
    let mut out = Vec::new();
    while !bytes.is_empty() {
        if bytes.len() < 4 {
            return Err("truncated length prefix".into());
        }
        let len = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
        let body = bytes.get(4..4 + len).ok_or("truncated record")?;
        out.push(postcard::from_bytes(body).map_err(|e| e.to_string())?);
        bytes = &bytes[4 + len..];
    }
    Ok(out)
}
