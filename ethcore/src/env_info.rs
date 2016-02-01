use util::*;
use header::BlockNumber;

/// Simple vector of hashes, should be at most 256 items large, can be smaller if being used
/// for a block whose number is less than 257.
pub type LastHashes = Vec<H256>;

/// Information concerning the execution environment for a message-call/contract-creation.
#[derive(Debug)]
pub struct EnvInfo {
	/// The block number.
	pub number: BlockNumber,
	/// The block author.
	pub author: Address,
	/// The block timestamp.
	pub timestamp: u64,
	/// The block difficulty.
	pub difficulty: U256,
	/// The block gas limit.
	pub gas_limit: U256,
	/// The last 256 block hashes.
	pub last_hashes: LastHashes,
	/// The gas used.
	pub gas_used: U256,
}

impl EnvInfo {
	/// Create empty env_info initialized with zeros
	pub fn new() -> EnvInfo {
		EnvInfo::default()
	}
}

impl Default for EnvInfo {
	fn default() -> Self {
		EnvInfo {
			number: 0,
			author: Address::new(),
			timestamp: 0,
			difficulty: x!(0),
			gas_limit: x!(0),
			last_hashes: vec![],
			gas_used: x!(0),
		}
	}
}

impl FromJson for EnvInfo {
	fn from_json(json: &Json) -> EnvInfo {
		let current_number: u64 = xjson!(&json["currentNumber"]);
		EnvInfo {
			number: current_number,
			author: xjson!(&json["currentCoinbase"]),
			difficulty: xjson!(&json["currentDifficulty"]),
			gas_limit: xjson!(&json["currentGasLimit"]),
			timestamp: xjson!(&json["currentTimestamp"]),
			last_hashes: (1..cmp::min(current_number + 1, 257)).map(|i| format!("{}", current_number - i).as_bytes().sha3()).collect(),
			gas_used: x!(0),
		}
	}
}

#[cfg(test)]
mod tests {
	extern crate rustc_serialize;

	use super::*;
	use rustc_serialize::*;
	use util::from_json::FromJson;
	use util::hash::*;
	use std::str::FromStr;

	#[test]
	fn it_serializes_form_json() {
		let env_info = EnvInfo::from_json(&json::Json::from_str(
r#"
	{
		"currentCoinbase": "0x000000f00000000f000000000000f00000000f00",
		"currentNumber": 1112339,
		"currentDifficulty": 50000,
		"currentGasLimit" : 40000,
		"currentTimestamp" : 1100
	}
"#
		).unwrap());

		assert_eq!(env_info.number, 1112339);
		assert_eq!(env_info.author, Address::from_str("000000f00000000f000000000000f00000000f00").unwrap());
		assert_eq!(env_info.gas_limit, x!(40000));
		assert_eq!(env_info.difficulty, x!(50000));
		assert_eq!(env_info.gas_used, x!(0));
	}

	#[test]
	fn it_can_be_created_as_default() {
		let default_env_info = EnvInfo::default();

		assert_eq!(default_env_info.difficulty, x!(0));
	}
}