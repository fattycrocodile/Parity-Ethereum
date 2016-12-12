// Copyright 2015, 2016 Parity Technologies (UK) Ltd.
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

//! Light client header chain.
//!
//! Unlike a full node's `BlockChain` this doesn't store much in the database.
//! It stores candidates for the last 2048-4096 blocks as well as CHT roots for
//! historical blocks all the way to the genesis.
//!
//! This is separate from the `BlockChain` for two reasons:
//!   - It stores only headers (and a pruned subset of them)
//!   - To allow for flexibility in the database layout once that's incorporated.
// TODO: use DB instead of memory.

use std::collections::{BTreeMap, HashMap};

use ethcore::header::Header;
use ethcore::error::BlockError;
use ethcore::ids::BlockId;
use ethcore::views::HeaderView;
use util::{Bytes, H256, U256, Mutex, RwLock};

/// Delay this many blocks before producing a CHT.
const CHT_DELAY: u64 = 2048;

/// Generate CHT roots of this size.
// TODO: move into more generic module.
const CHT_SIZE: u64 = 2048;

#[derive(Debug, Clone)]
struct BestBlock {
	hash: H256,
	number: u64,
	total_difficulty: U256,
}

// candidate block description.
struct Candidate {
	hash: H256,
	parent_hash: H256,
	total_difficulty: U256,
}

struct Entry {
	candidates: Vec<Candidate>,
	canonical_hash: H256,
}

/// Header chain. See module docs for more details.
pub struct HeaderChain {
	genesis_header: Bytes, // special-case the genesis.
	candidates: RwLock<BTreeMap<u64, Entry>>,
	headers: RwLock<HashMap<H256, Bytes>>,
	best_block: RwLock<BestBlock>,
	cht_roots: Mutex<Vec<H256>>,
}

impl HeaderChain {
	/// Create a new header chain given this genesis block.
	pub fn new(genesis: &[u8]) -> Self {
		let g_view = HeaderView::new(genesis);

		HeaderChain {
			genesis_header: genesis.to_owned(),
			best_block: RwLock::new(BestBlock {
				hash: g_view.hash(),
				number: 0,
				total_difficulty: g_view.difficulty(),
			}),
			candidates: RwLock::new(BTreeMap::new()),
			headers: RwLock::new(HashMap::new()),
			cht_roots: Mutex::new(Vec::new()),
		}
	}

	/// Insert a pre-verified header.
	pub fn insert(&self, header: Bytes) -> Result<(), BlockError> {
		let view = HeaderView::new(&header);
		let hash = view.hash();
		let number = view.number();
		let parent_hash = view.parent_hash();

		// find parent details.
		let parent_td = {
			if number == 1 {
				let g_view = HeaderView::new(&self.genesis_header);
				g_view.difficulty()
			} else {
				let maybe_td = self.candidates.read().get(&(number - 1))
					.and_then(|entry| entry.candidates.iter().find(|c| c.hash == parent_hash))
					.map(|c| c.total_difficulty);

				match maybe_td {
					Some(td) => td,
					None => return Err(BlockError::UnknownParent(parent_hash)),
				}
			}
		};

		let total_difficulty = parent_td + view.difficulty();

		// insert headers and candidates entries.
		let mut candidates = self.candidates.write();
		candidates.entry(number).or_insert_with(|| Entry { candidates: Vec::new(), canonical_hash: hash})
			.candidates.push(Candidate {
				hash: hash,
				parent_hash: parent_hash,
				total_difficulty: total_difficulty,
		});

		self.headers.write().insert(hash, header.clone());

		// reorganize ancestors so canonical entries are first in their
		// respective candidates vectors.
		if self.best_block.read().total_difficulty < total_difficulty {
			let mut canon_hash = hash;
			for (_, entry) in candidates.iter_mut().rev().skip_while(|&(height, _)| *height > number) {
				if entry.canonical_hash == canon_hash { break; }

				let canon = entry.candidates.iter().find(|x| x.hash == canon_hash)
					.expect("blocks are only inserted if parent is present; or this is the block we just added; qed");

				// what about reorgs > CHT_SIZE + CHT_DELAY?
				canon_hash = canon.parent_hash;
			}

			*self.best_block.write() = BestBlock {
				hash: hash,
				number: number,
				total_difficulty: total_difficulty,
			};

			// produce next CHT root if it's time.
			let earliest_era = *candidates.keys().next().expect("at least one era just created; qed");
			if earliest_era + CHT_DELAY + CHT_SIZE < number {
				let values: Vec<_> = (0..CHT_SIZE).map(|x| x + earliest_era)
					.map(|x| candidates.remove(&x).map(|entry| (x, entry)))
					.map(|x| x.expect("all eras stored are sequential with no gaps; qed"))
					.map(|(x, entry)| (::rlp::encode(&x), ::rlp::encode(&entry.canonical_hash)))
					.map(|(k, v)| (k.to_vec(), v.to_vec()))
					.collect();

				let cht_root = ::util::triehash::trie_root(values);
				debug!(target: "chain", "Produced CHT {} root: {:?}", (earliest_era - 1) % CHT_SIZE, cht_root);

				self.cht_roots.lock().push(cht_root);
			}
		}

		Ok(())
	}

	/// Get a block header. In the case of query by number, only canonical blocks
	/// will be returned.
	pub fn block_header(&self, id: BlockId) -> Option<Bytes> {
		match id {
			BlockId::Earliest | BlockId::Number(0) => Some(self.genesis_header.clone()),
			BlockId::Hash(hash) => self.headers.read().get(&hash).map(|x| x.to_vec()),
			BlockId::Number(num) => {
				if self.best_block.read().number < num { return None }

				self.candidates.read().get(&num).map(|entry| entry.canonical_hash)
					.and_then(|hash| self.headers.read().get(&hash).map(|x| x.to_vec()))
			}
			BlockId::Latest | BlockId::Pending => {
				let hash = self.best_block.read().hash;
				self.headers.read().get(&hash).map(|x| x.to_vec())
			}
		}
	}

}
