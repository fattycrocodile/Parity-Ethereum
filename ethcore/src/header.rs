// Copyright 2015, 2016 Ethcore (UK) Ltd.
// This file is part of Parity.

// Parity is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Parity is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Parity.  If not, see <http://www.gnu.org/licenses/>.

//! Block header.

use util::*;
use basic_types::*;
use time::now_utc;

/// Type for Block number
pub type BlockNumber = u64;

/// A block header.
///
/// Reflects the specific RLP fields of a block in the chain with additional room for the seal
/// which is non-specific.
///
/// Doesn't do all that much on its own.
#[derive(Debug, Clone)]
pub struct Header {
	// TODO: make all private.
	/// Parent hash.
	pub parent_hash: H256,
	/// Block timestamp.
	pub timestamp: u64,
	/// Block number.
	pub number: BlockNumber,
	/// Block author.
	pub author: Address,

	/// Transactions root.
	pub transactions_root: H256,
	/// Block uncles hash.
	pub uncles_hash: H256,
	/// Block extra data.
	pub extra_data: Bytes,

	/// State root.
	pub state_root: H256,
	/// Block receipts root.
	pub receipts_root: H256,
	/// Block bloom.
	pub log_bloom: LogBloom,
	/// Gas used for contracts execution.
	pub gas_used: U256,
	/// Block gas limit.
	pub gas_limit: U256,

	/// Block difficulty.
	pub difficulty: U256,
	/// Block seal.
	pub seal: Vec<Bytes>,

	/// The memoized hash of the RLP representation *including* the seal fields.
	pub hash: RefCell<Option<H256>>,
	/// The memoized hash of the RLP representation *without* the seal fields.
	pub bare_hash: RefCell<Option<H256>>,
}

impl Default for Header {
	fn default() -> Self {
		Header {
			parent_hash: ZERO_H256.clone(),
			timestamp: 0,
			number: 0,
			author: ZERO_ADDRESS.clone(),

			transactions_root: SHA3_NULL_RLP,
			uncles_hash: SHA3_EMPTY_LIST_RLP,
			extra_data: vec![],

			state_root: SHA3_NULL_RLP,
			receipts_root: SHA3_NULL_RLP,
			log_bloom: ZERO_LOGBLOOM.clone(),
			gas_used: ZERO_U256,
			gas_limit: ZERO_U256,

			difficulty: ZERO_U256,
			seal: vec![],
			hash: RefCell::new(None),
			bare_hash: RefCell::new(None),
		}
	}
}

impl Header {
	/// Create a new, default-valued, header.
	pub fn new() -> Self {
		Self::default()
	}

	/// Get the parent_hash field of the header.
	pub fn parent_hash(&self) -> &H256 { &self.parent_hash }
	/// Get the timestamp field of the header.
	pub fn timestamp(&self) -> u64 { self.timestamp }
	/// Get the number field of the header.
	pub fn number(&self) -> BlockNumber { self.number }
	/// Get the author field of the header.
	pub fn author(&self) -> &Address { &self.author }

	/// Get the extra data field of the header.
	pub fn extra_data(&self) -> &Bytes { &self.extra_data }

	/// Get the state root field of the header.
	pub fn state_root(&self) -> &H256 { &self.state_root }
	/// Get the receipts root field of the header.
	pub fn receipts_root(&self) -> &H256 { &self.receipts_root }
	/// Get the gas limit field of the header.
	pub fn gas_limit(&self) -> &U256 { &self.gas_limit }

	/// Get the difficulty field of the header.
	pub fn difficulty(&self) -> &U256 { &self.difficulty }
	/// Get the seal field of the header.
	pub fn seal(&self) -> &Vec<Bytes> { &self.seal }

	// TODO: seal_at, set_seal_at &c.

	/// Set the number field of the header.
	pub fn set_parent_hash(&mut self, a: H256) { self.parent_hash = a; self.note_dirty(); }
	/// Set the timestamp field of the header.
	pub fn set_timestamp(&mut self, a: u64) { self.timestamp = a; self.note_dirty(); }
	/// Set the timestamp field of the header to the current time.
	pub fn set_timestamp_now(&mut self, but_later_than: u64) { self.timestamp = max(now_utc().to_timespec().sec as u64, but_later_than + 1); self.note_dirty(); }
	/// Set the number field of the header.
	pub fn set_number(&mut self, a: BlockNumber) { self.number = a; self.note_dirty(); }
	/// Set the author field of the header.
	pub fn set_author(&mut self, a: Address) { if a != self.author { self.author = a; self.note_dirty(); } }

	/// Set the extra data field of the header.
	pub fn set_extra_data(&mut self, a: Bytes) { if a != self.extra_data { self.extra_data = a; self.note_dirty(); } }

	/// Set the gas used field of the header.
	pub fn set_gas_used(&mut self, a: U256) { self.gas_used = a; self.note_dirty(); }
	/// Set the gas limit field of the header.
	pub fn set_gas_limit(&mut self, a: U256) { self.gas_limit = a; self.note_dirty(); }

	/// Set the difficulty field of the header.
	pub fn set_difficulty(&mut self, a: U256) { self.difficulty = a; self.note_dirty(); }
	/// Set the seal field of the header.
	pub fn set_seal(&mut self, a: Vec<Bytes>) { self.seal = a; self.note_dirty(); }

	/// Get the hash of this header (sha3 of the RLP).
	pub fn hash(&self) -> H256 {
 		let mut hash = self.hash.borrow_mut();
 		match &mut *hash {
 			&mut Some(ref h) => h.clone(),
 			hash @ &mut None => {
 				*hash = Some(self.rlp_sha3(Seal::With));
 				hash.as_ref().unwrap().clone()
 			}
		}
	}

	/// Get the hash of the header excluding the seal
	pub fn bare_hash(&self) -> H256 {
		let mut hash = self.bare_hash.borrow_mut();
		match &mut *hash {
			&mut Some(ref h) => h.clone(),
			hash @ &mut None => {
				*hash = Some(self.rlp_sha3(Seal::Without));
				hash.as_ref().unwrap().clone()
			}
		}
	}

	/// Note that some fields have changed. Resets the memoised hash.
	pub fn note_dirty(&self) {
 		*self.hash.borrow_mut() = None;
 		*self.bare_hash.borrow_mut() = None;
	}

	// TODO: make these functions traity 
	/// Place this header into an RLP stream `s`, optionally `with_seal`.
	pub fn stream_rlp(&self, s: &mut RlpStream, with_seal: Seal) {
		s.begin_list(13 + match with_seal { Seal::With => self.seal.len(), _ => 0 });
		s.append(&self.parent_hash);
		s.append(&self.uncles_hash);
		s.append(&self.author);
		s.append(&self.state_root);
		s.append(&self.transactions_root);
		s.append(&self.receipts_root);
		s.append(&self.log_bloom);
		s.append(&self.difficulty);
		s.append(&self.number);
		s.append(&self.gas_limit);
		s.append(&self.gas_used);
		s.append(&self.timestamp);
		s.append(&self.extra_data);
		if let Seal::With = with_seal {
			for b in &self.seal { 
				s.append_raw(&b, 1); 
			}
		}
	}

	/// Get the RLP of this header, optionally `with_seal`.
	pub fn rlp(&self, with_seal: Seal) -> Bytes {
		let mut s = RlpStream::new();
		self.stream_rlp(&mut s, with_seal);
		s.out()
	}

	/// Get the SHA3 (Keccak) of this header, optionally `with_seal`.
	pub fn rlp_sha3(&self, with_seal: Seal) -> H256 { self.rlp(with_seal).sha3() }
}

impl Decodable for Header {
	fn decode<D>(decoder: &D) -> Result<Self, DecoderError> where D: Decoder {
		let r = decoder.as_rlp();

		let mut blockheader = Header {
			parent_hash: try!(r.val_at(0)),
			uncles_hash: try!(r.val_at(1)),
			author: try!(r.val_at(2)),
			state_root: try!(r.val_at(3)),
			transactions_root: try!(r.val_at(4)),
			receipts_root: try!(r.val_at(5)),
			log_bloom: try!(r.val_at(6)),
			difficulty: try!(r.val_at(7)),
			number: try!(r.val_at(8)),
			gas_limit: try!(r.val_at(9)),
			gas_used: try!(r.val_at(10)),
			timestamp: try!(r.val_at(11)),
			extra_data: try!(r.val_at(12)),
			seal: vec![],
			hash: RefCell::new(Some(r.as_raw().sha3())),
			bare_hash: RefCell::new(None),
		};

		for i in 13..r.item_count() {
			blockheader.seal.push(try!(r.at(i)).as_raw().to_vec())
		}

		Ok(blockheader)
	}
}

impl Encodable for Header {
	fn rlp_append(&self, s: &mut RlpStream) {
		self.stream_rlp(s, Seal::With);
	}
}

#[cfg(test)]
mod tests {
}
