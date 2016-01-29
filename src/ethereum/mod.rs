//! Ethereum protocol module.
//!
//! Contains all Ethereum network specific stuff, such as denominations and
//! consensus specifications.

/// TODO [Gav Wood] Please document me
pub mod ethash;
/// TODO [Gav Wood] Please document me
pub mod denominations;

pub use self::ethash::*;
pub use self::denominations::*;

use super::spec::*;

/// Create a new Olympic chain spec.
pub fn new_olympic() -> Spec { Spec::from_json_utf8(include_bytes!("../../res/ethereum/olympic.json")) }

/// Create a new Frontier mainnet chain spec.
pub fn new_frontier() -> Spec { Spec::from_json_utf8(include_bytes!("../../res/ethereum/frontier.json")) }

/// Create a new Frontier chain spec as though it never changes to Homestead.
pub fn new_frontier_test() -> Spec { Spec::from_json_utf8(include_bytes!("../../res/ethereum/frontier_test.json")) }

/// Create a new Homestead chain spec as though it never changed from Frontier.
pub fn new_homestead_test() -> Spec { Spec::from_json_utf8(include_bytes!("../../res/ethereum/homestead_test.json")) }

/// Create a new Frontier main net chain spec without genesis accounts.
pub fn new_mainnet_like() -> Spec { Spec::from_json_utf8(include_bytes!("../../res/ethereum/frontier_like_test.json")) }

/// Create a new Morden chain spec.
pub fn new_morden() -> Spec { Spec::from_json_utf8(include_bytes!("../../res/ethereum/morden.json")) }

#[cfg(test)]
mod tests {
	use common::*;
	use state::*;
	use engine::*;
	use super::*;

	#[test]
	fn ensure_db_good() {
		let engine = new_morden().to_engine().unwrap();
		let genesis_header = engine.spec().genesis_header();
		let mut db = JournalDB::new_temp();
		engine.spec().ensure_db_good(&mut db);
		let s = State::from_existing(db.clone(), genesis_header.state_root.clone(), engine.account_start_nonce());
		assert_eq!(s.balance(&address_from_hex("0000000000000000000000000000000000000001")), U256::from(1u64));
		assert_eq!(s.balance(&address_from_hex("0000000000000000000000000000000000000002")), U256::from(1u64));
		assert_eq!(s.balance(&address_from_hex("0000000000000000000000000000000000000003")), U256::from(1u64));
		assert_eq!(s.balance(&address_from_hex("0000000000000000000000000000000000000004")), U256::from(1u64));
		assert_eq!(s.balance(&address_from_hex("102e61f5d8f9bc71d0ad4a084df4e65e05ce0e1c")), U256::from(1u64) << 200);
		assert_eq!(s.balance(&address_from_hex("0000000000000000000000000000000000000000")), U256::from(0u64));
	}

	#[test]
	fn morden() {
		let morden = new_morden();

		assert_eq!(morden.state_root(), H256::from_str("f3f4696bbf3b3b07775128eb7a3763279a394e382130f27c21e70233e04946a9").unwrap());
		let genesis = morden.genesis_block();
		assert_eq!(BlockView::new(&genesis).header_view().sha3(), H256::from_str("0cd786a2425d16f152c658316c423e6ce1181e15c3295826d7c9904cba9ce303").unwrap());

		let _ = morden.to_engine();
	}

	#[test]
	fn frontier() {
		let frontier = new_frontier();

		assert_eq!(frontier.state_root(), H256::from_str("d7f8974fb5ac78d9ac099b9ad5018bedc2ce0a72dad1827a1709da30580f0544").unwrap());
		let genesis = frontier.genesis_block();
		assert_eq!(BlockView::new(&genesis).header_view().sha3(), H256::from_str("d4e56740f876aef8c010b86a40d5f56745a118d0906a34e69aec8c0db1cb8fa3").unwrap());

		let _ = frontier.to_engine();
	}
}
