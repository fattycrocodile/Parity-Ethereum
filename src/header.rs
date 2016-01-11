use util::*;
use basic_types::*;
use time::now_utc;

pub type BlockNumber = u64;

/// A block header.
///
/// Reflects the specific RLP fields of a block in the chain with additional room for the seal
/// which is non-specific.
///
/// Doesn't do all that much on its own.
#[derive(Debug)]
pub struct Header {
	// TODO: make all private.
	pub parent_hash: H256,
	pub timestamp: u64,
	pub number: BlockNumber,
	pub author: Address,

	pub transactions_root: H256,
	pub uncles_hash: H256,
	pub extra_data: Bytes,

	pub state_root: H256,
	pub receipts_root: H256,
	pub log_bloom: LogBloom,
	pub gas_used: U256,
	pub gas_limit: U256,

	pub difficulty: U256,
	pub seal: Vec<Bytes>,

	pub hash: RefCell<Option<H256>>,
}

pub enum Seal {
	With,
	Without,
}

impl Header {
	/// Create a new, default-valued, header.
	pub fn new() -> Header {
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
		}
	}

	pub fn number(&self) -> BlockNumber { self.number }
	pub fn timestamp(&self) -> u64 { self.timestamp }
	pub fn author(&self) -> &Address { &self.author }

	pub fn extra_data(&self) -> &Bytes { &self.extra_data }

	pub fn seal(&self) -> &Vec<Bytes> { &self.seal }

	// TODO: seal_at, set_seal_at &c.

	pub fn set_number(&mut self, a: BlockNumber) { self.number = a; self.note_dirty(); }
	pub fn set_timestamp(&mut self, a: u64) { self.timestamp = a; self.note_dirty(); }
	pub fn set_timestamp_now(&mut self) { self.timestamp = now_utc().to_timespec().sec as u64; self.note_dirty(); }
	pub fn set_author(&mut self, a: Address) { if a != self.author { self.author = a; self.note_dirty(); } }

	pub fn set_extra_data(&mut self, a: Bytes) { if a != self.extra_data { self.extra_data = a; self.note_dirty(); } }

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

	/// Note that some fields have changed. Resets the memoised hash.
	pub fn note_dirty(&self) {
 		*self.hash.borrow_mut() = None;
	}

	// TODO: get hash without seal.

	// TODO: make these functions traity 
	pub fn stream_rlp(&self, s: &mut RlpStream, with_seal: Seal) {
		s.append_list(13 + match with_seal { Seal::With => self.seal.len(), _ => 0 });
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
		match with_seal {
			Seal::With => for b in self.seal.iter() { s.append_raw(&b, 1); },
			_ => {}
		}
	}

	pub fn rlp(&self, with_seal: Seal) -> Bytes {
		let mut s = RlpStream::new();
		self.stream_rlp(&mut s, with_seal);
		s.out()
	}

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
			hash: RefCell::new(Some(r.as_raw().sha3()))
		};

		for i in 13..r.item_count() {
			blockheader.seal.push(try!(r.at(i)).as_raw().to_vec())
		}

		Ok(blockheader)
	}
}

impl Encodable for Header {
	fn encode<E>(&self, encoder: &mut E) where E: Encoder {
		encoder.emit_list(| e | {
			self.parent_hash.encode(e);
			self.uncles_hash.encode(e);
			self.author.encode(e);
			self.state_root.encode(e);
			self.transactions_root.encode(e);
			self.receipts_root.encode(e);
			self.log_bloom.encode(e);
			self.difficulty.encode(e);
			self.number.encode(e);
			self.gas_limit.encode(e);
			self.gas_used.encode(e);
			self.timestamp.encode(e);
			self.extra_data.encode(e);

			for b in self.seal.iter() {
				e.emit_raw(&b);
			}
		})
	}
}

#[cfg(test)]
mod tests {
}
