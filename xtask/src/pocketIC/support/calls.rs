use anyhow::{anyhow, Result};
use candid::{decode_one, encode_one, CandidType, Deserialize, Principal};
use pocket_ic::PocketIc;

pub fn update_bytes<R: for<'de> Deserialize<'de> + CandidType>(
    pic: &PocketIc,
    canister: Principal,
    sender: Principal,
    method: &str,
    bytes: Vec<u8>,
) -> Result<R> {
    let reply = pic
        .update_call(canister, sender, method, bytes)
        .map_err(|e| anyhow!("update_call {method} rejected: {e:?}"))?;
    decode_one(&reply).map_err(Into::into)
}

pub fn update_one<A: CandidType, R: for<'de> Deserialize<'de> + CandidType>(
    pic: &PocketIc,
    canister: Principal,
    sender: Principal,
    method: &str,
    arg: A,
) -> Result<R> {
    update_bytes(pic, canister, sender, method, encode_one(arg)?)
}

pub fn update_noargs<R: for<'de> Deserialize<'de> + CandidType>(
    pic: &PocketIc,
    canister: Principal,
    sender: Principal,
    method: &str,
) -> Result<R> {
    update_one(pic, canister, sender, method, ())
}

pub fn query_one<A: CandidType, R: for<'de> Deserialize<'de> + CandidType>(
    pic: &PocketIc,
    canister: Principal,
    sender: Principal,
    method: &str,
    arg: A,
) -> Result<R> {
    let reply = pic
        .query_call(canister, sender, method, encode_one(arg)?)
        .map_err(|e| anyhow!("query_call {method} rejected: {e:?}"))?;
    decode_one(&reply).map_err(Into::into)
}

pub fn tick_n(pic: &PocketIc, n: usize) {
    for _ in 0..n {
        pic.tick();
    }
}
