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

//! Disk-backed HashDB implementation.

use common::*;
use rlp::*;
use hashdb::*;
use memorydb::*;
use rocksdb::{DB, Writable, WriteBatch, IteratorMode};
#[cfg(test)]
use std::env;

/// Implementation of the HashDB trait for a disk-backed database with a memory overlay
/// and latent-removal semantics.
///
/// Like OverlayDB, there is a memory overlay; `commit()` must be called in order to 
/// write operations out to disk. Unlike OverlayDB, `remove()` operations do not take effect
/// immediately. Rather some age (based on a linear but arbitrary metric) must pass before
/// the removals actually take effect.
pub struct JournalDB {
	overlay: MemoryDB,
	backing: Arc<DB>,
	counters: Arc<RwLock<HashMap<H256, i32>>>,
}

impl Clone for JournalDB {
	fn clone(&self) -> JournalDB {
		JournalDB {
			overlay: MemoryDB::new(),
			backing: self.backing.clone(),
			counters: self.counters.clone(),
		}
	}
}

const LAST_ERA_KEY : [u8; 4] = [ b'l', b'a', b's', b't' ]; 
const VERSION_KEY : [u8; 4] = [ b'j', b'v', b'e', b'r' ]; 

const DB_VERSION: u32 = 1;

impl JournalDB {
	/// Create a new instance given a `backing` database.
	pub fn new(backing: DB) -> JournalDB {
		let db = Arc::new(backing);
		JournalDB::new_with_arc(db)
	}

	/// Create a new instance given a shared `backing` database.
	pub fn new_with_arc(backing: Arc<DB>) -> JournalDB {
		if backing.iterator(IteratorMode::Start).next().is_some() {
			match backing.get(&VERSION_KEY).map(|d| d.map(|v| decode::<u32>(&v))) {
				Ok(Some(DB_VERSION)) => {},
				v => panic!("Incompatible DB version, expected {}, got {:?}", DB_VERSION, v)
			}
		} else {
			backing.put(&VERSION_KEY, &encode(&DB_VERSION)).expect("Error writing version to database");
		}
		let counters = JournalDB::read_counters(&backing);
		JournalDB {
			overlay: MemoryDB::new(),
			backing: backing,
			counters: Arc::new(RwLock::new(counters)),
		}
	}

	/// Create a new instance with an anonymous temporary database.
	#[cfg(test)]
	pub fn new_temp() -> JournalDB {
		let mut dir = env::temp_dir();
		dir.push(H32::random().hex());
		Self::new(DB::open_default(dir.to_str().unwrap()).unwrap())
	}

	/// Check if this database has any commits
	pub fn is_empty(&self) -> bool {
		self.backing.get(&LAST_ERA_KEY).expect("Low level database error").is_none()
	}

	fn morph_key(key: &H256, index: u8) -> Bytes {
		let mut ret = key.bytes().to_owned();
		ret.push(index);
		ret
	}

	// The next three are valid only as long as there is an insert operation of `key` in the journal.
	fn set_already_in(batch: &WriteBatch, key: &H256) { batch.put(&Self::morph_key(key, 0), &[1u8]); }
	fn reset_already_in(batch: &WriteBatch, key: &H256) { batch.delete(&Self::morph_key(key, 0)); }
	fn is_already_in(backing: &DB, key: &H256) -> bool {
		backing.get(&Self::morph_key(key, 0)).expect("Low-level database error. Some issue with your hard disk?").is_some()
	}

	fn insert_keys(inserts: &Vec<(H256, Bytes)>, backing: &DB, counters: &mut HashMap<H256, i32>, batch: &WriteBatch) {
		for &(ref h, ref d) in inserts {
			if let Some(c) = counters.get_mut(h) {
				// already counting. increment.
				*c += 1;
				continue;
			}

			// this is the first entry for this node in the journal.
			if backing.get(&h.bytes()).expect("Low-level database error. Some issue with your hard disk?").is_some() {
				// already in the backing DB. start counting, and remember it was already in.
				Self::set_already_in(batch, &h);
				counters.insert(h.clone(), 1);
				continue;
			}

			// Gets removed when a key leaves the journal, so should never be set when we're placing a new key.
			//Self::reset_already_in(&h);
			assert!(!Self::is_already_in(backing, &h));
			batch.put(&h.bytes(), d);
		}
	}

	fn replay_keys(inserts: &Vec<H256>, backing: &DB, counters: &mut HashMap<H256, i32>) {
		for h in inserts {
			if let Some(c) = counters.get_mut(h) {
				// already counting. increment.
				*c += 1;
				continue;
			}

			// this is the first entry for this node in the journal.
			// it is initialised to 1 if it was already in.
			counters.insert(h.clone(), if Self::is_already_in(backing, h) {1} else {0});
		}
	}

	fn kill_keys(deletes: Vec<H256>, counters: &mut HashMap<H256, i32>, batch: &WriteBatch) {
		for h in deletes.into_iter() {
			let mut n: Option<i32> = None;
			if let Some(c) = counters.get_mut(&h) {
				if *c > 1 {
					*c -= 1;
					continue;
				} else {
					n = Some(*c);
				}
			}
			match &n {
				&Some(i) if i == 1 => {
					counters.remove(&h);
					Self::reset_already_in(batch, &h);
				}
				&None => {
					// Gets removed when moving from 1 to 0 additional refs. Should never be here at 0 additional refs.
					//assert!(!Self::is_already_in(db, &h));
					batch.delete(&h.bytes());
				}
				_ => panic!("Invalid value in counters: {:?}", n),
			}
		}
	}

	/// Commit all recent insert operations and historical removals from the old era
	/// to the backing database.
	pub fn commit(&mut self, now: u64, id: &H256, end: Option<(u64, H256)>) -> Result<u32, UtilError> {
		// journal format: 
		// [era, 0] => [ id, [insert_0, ...], [remove_0, ...] ]
		// [era, 1] => [ id, [insert_0, ...], [remove_0, ...] ]
		// [era, n] => [ ... ]

		// TODO: store reclaim_period.

		// When we make a new commit, we make a journal of all blocks in the recent history and record
		// all keys that were inserted and deleted. The journal is ordered by era; multiple commits can
		// share the same era. This forms a data structure similar to a queue but whose items are tuples.
		// By the time comes to remove a tuple from the queue (i.e. then the era passes from recent history
		// into ancient history) then only one commit from the tuple is considered canonical. This commit
		// is kept in the main backing database, whereas any others from the same era are reverted.
		// 
		// It is possible that a key, properly available in the backing database be deleted and re-inserted
		// in the recent history queue, yet have both operations in commits that are eventually non-canonical.
		// To avoid the original, and still required, key from being deleted, we maintain a reference count
		// which includes an original key, if any.
		// 
		// The semantics of the `counter` are:
		// insert key k:
		//   counter already contains k: count += 1
		//   counter doesn't contain k:
		//     backing db contains k: count = 1
		//     backing db doesn't contain k: insert into backing db, count = 0
		// delete key k:
		//   counter contains k (count is asserted to be non-zero): 
		//     count > 1: counter -= 1
		//     count == 1: remove counter
		//     count == 0: remove key from backing db
		//   counter doesn't contain k: remove key from backing db
		//
		// Practically, this means that for each commit block turning from recent to ancient we do the
		// following:
		// is_canonical:
		//   inserts: Ignored (left alone in the backing database).
		//   deletes: Enacted; however, recent history queue is checked for ongoing references. This is
		//            reduced as a preference to deletion from the backing database.
		// !is_canonical:
		//   inserts: Reverted; however, recent history queue is checked for ongoing references. This is
		//            reduced as a preference to deletion from the backing database.
		//   deletes: Ignored (they were never inserted).
		//

		// record new commit's details.
		let batch = WriteBatch::new();
		let mut counters = self.counters.write().unwrap();
		{
			let mut index = 0usize;
			let mut last;

			while try!(self.backing.get({
				let mut r = RlpStream::new_list(2);
				r.append(&now);
				r.append(&index);
				last = r.drain();
				&last
			})).is_some() {
				index += 1;
			}

			let drained = self.overlay.drain();
			let removes: Vec<H256> = drained
				.iter()
				.filter_map(|(ref k, &(_, ref c))| if *c < 0 {Some(k.clone())} else {None}).cloned()
				.collect();
			let inserts: Vec<(H256, Bytes)> = drained
				.into_iter()
				.filter_map(|(k, (v, r))| if r > 0 { assert!(r == 1); Some((k, v)) } else { assert!(r >= -1); None })
				.collect();

			let mut r = RlpStream::new_list(3);
			r.append(id);

			// Process the new inserts.
			// We use the inserts for three things. For each:
			// - we place into the backing DB or increment the counter if already in;
			// - we note in the backing db that it was already in;
			// - we write the key into our journal for this block;

			r.begin_list(inserts.len());
			inserts.iter().foreach(|&(k, _)| {r.append(&k);});
			r.append(&removes);
			Self::insert_keys(&inserts, &self.backing, &mut counters, &batch);
			try!(batch.put(&last, r.as_raw()));
		}

		// apply old commits' details
		if let Some((end_era, canon_id)) = end {
			let mut index = 0usize;
			let mut last;
			let mut to_remove: Vec<H256> = Vec::new();
			while let Some(rlp_data) = try!(self.backing.get({
				let mut r = RlpStream::new_list(2);
				r.append(&end_era);
				r.append(&index);
				last = r.drain();
				&last
			})) {
				let rlp = Rlp::new(&rlp_data);
				let inserts: Vec<H256> = rlp.val_at(1);
				let deletes: Vec<H256> = rlp.val_at(2);
				// Collect keys to be removed. These are removed keys for canonical block, inserted for non-canonical
				Self::kill_keys(if canon_id == rlp.val_at(0) {deletes} else {inserts}, &mut counters, &batch);
				try!(batch.delete(&last));
				index += 1;
			}
			try!(batch.put(&LAST_ERA_KEY, &encode(&end_era)));
			trace!("JournalDB: delete journal for time #{}.{}, (canon was {})", end_era, index, canon_id);
		}

		try!(self.backing.write(batch));
//		trace!("JournalDB::commit() deleted {} nodes", deletes);
		Ok(0)
	}

	fn payload(&self, key: &H256) -> Option<Bytes> {
		self.backing.get(&key.bytes()).expect("Low-level database error. Some issue with your hard disk?").map(|v| v.to_vec())
	}

	fn read_counters(db: &DB) -> HashMap<H256, i32> {
		let mut counters = HashMap::new();
		if let Some(val) = db.get(&LAST_ERA_KEY).expect("Low-level database error.") {
			let mut era = decode::<u64>(&val) + 1;
			loop {
				let mut index = 0usize;
				while let Some(rlp_data) = db.get({
					let mut r = RlpStream::new_list(2);
					r.append(&era);
					r.append(&index);
					&r.drain()
				}).expect("Low-level database error.") {
					let rlp = Rlp::new(&rlp_data);
					let inserts: Vec<H256> = rlp.val_at(1);
					Self::replay_keys(&inserts, db, &mut counters);
					index += 1;
				};
				if index == 0 {
					break;
				}
				era += 1;
			}
		}
		trace!("Recovered {} counters", counters.len());
		counters
	}
}

impl HashDB for JournalDB {
	fn keys(&self) -> HashMap<H256, i32> { 
		let mut ret: HashMap<H256, i32> = HashMap::new();
		for (key, _) in self.backing.iterator(IteratorMode::Start) {
			let h = H256::from_slice(key.deref());
			ret.insert(h, 1);
		}

		for (key, refs) in self.overlay.keys().into_iter() {
			let refs = *ret.get(&key).unwrap_or(&0) + refs;
			ret.insert(key, refs);
		}
		ret
	}

	fn lookup(&self, key: &H256) -> Option<&[u8]> { 
		let k = self.overlay.raw(key);
		match k {
			Some(&(ref d, rc)) if rc > 0 => Some(d),
			_ => {
				if let Some(x) = self.payload(key) {
					Some(&self.overlay.denote(key, x).0)
				}
				else {
					None
				}
			}
		}
	}

	fn exists(&self, key: &H256) -> bool { 
		self.lookup(key).is_some()
	}

	fn insert(&mut self, value: &[u8]) -> H256 { 
		self.overlay.insert(value)
	}
	fn emplace(&mut self, key: H256, value: Bytes) {
		self.overlay.emplace(key, value); 
	}
	fn kill(&mut self, key: &H256) { 
		self.overlay.kill(key); 
	}
}

#[cfg(test)]
mod tests {
	use common::*;
	use super::*;
	use hashdb::*;

	#[test]
	fn insert_same_in_fork() {
		// history is 1
		let mut jdb = JournalDB::new_temp();

		let x = jdb.insert(b"X");
		jdb.commit(1, &b"1".sha3(), None).unwrap();
		jdb.commit(2, &b"2".sha3(), None).unwrap();
		jdb.commit(3, &b"1002a".sha3(), Some((1, b"1".sha3()))).unwrap();
		jdb.commit(4, &b"1003a".sha3(), Some((2, b"2".sha3()))).unwrap();

		jdb.remove(&x);
		jdb.commit(3, &b"1002b".sha3(), Some((1, b"1".sha3()))).unwrap();
		let x = jdb.insert(b"X");
		jdb.commit(4, &b"1003b".sha3(), Some((2, b"2".sha3()))).unwrap();

		jdb.commit(5, &b"1004a".sha3(), Some((3, b"1002a".sha3()))).unwrap();
		jdb.commit(6, &b"1005a".sha3(), Some((4, b"1003a".sha3()))).unwrap();

		assert!(jdb.exists(&x));
	}

	#[test]
	fn long_history() {
		// history is 3
		let mut jdb = JournalDB::new_temp();
		let h = jdb.insert(b"foo");
		jdb.commit(0, &b"0".sha3(), None).unwrap();
		assert!(jdb.exists(&h));
		jdb.remove(&h);
		jdb.commit(1, &b"1".sha3(), None).unwrap();
		assert!(jdb.exists(&h));
		jdb.commit(2, &b"2".sha3(), None).unwrap();
		assert!(jdb.exists(&h));
		jdb.commit(3, &b"3".sha3(), Some((0, b"0".sha3()))).unwrap();
		assert!(jdb.exists(&h));
		jdb.commit(4, &b"4".sha3(), Some((1, b"1".sha3()))).unwrap();
		assert!(!jdb.exists(&h));
	}

	#[test]
	fn complex() {
		// history is 1
		let mut jdb = JournalDB::new_temp();

		let foo = jdb.insert(b"foo");
		let bar = jdb.insert(b"bar");
		jdb.commit(0, &b"0".sha3(), None).unwrap();
		assert!(jdb.exists(&foo));
		assert!(jdb.exists(&bar));

		jdb.remove(&foo);
		jdb.remove(&bar);
		let baz = jdb.insert(b"baz");
		jdb.commit(1, &b"1".sha3(), Some((0, b"0".sha3()))).unwrap();
		assert!(jdb.exists(&foo));
		assert!(jdb.exists(&bar));
		assert!(jdb.exists(&baz));

		let foo = jdb.insert(b"foo");
		jdb.remove(&baz);
		jdb.commit(2, &b"2".sha3(), Some((1, b"1".sha3()))).unwrap();
		assert!(jdb.exists(&foo));
		assert!(!jdb.exists(&bar));
		assert!(jdb.exists(&baz));

		jdb.remove(&foo);
		jdb.commit(3, &b"3".sha3(), Some((2, b"2".sha3()))).unwrap();
		assert!(jdb.exists(&foo));
		assert!(!jdb.exists(&bar));
		assert!(!jdb.exists(&baz));

		jdb.commit(4, &b"4".sha3(), Some((3, b"3".sha3()))).unwrap();
		assert!(!jdb.exists(&foo));
		assert!(!jdb.exists(&bar));
		assert!(!jdb.exists(&baz));
	}

	#[test]
	fn fork() {
		// history is 1
		let mut jdb = JournalDB::new_temp();

		let foo = jdb.insert(b"foo");
		let bar = jdb.insert(b"bar");
		jdb.commit(0, &b"0".sha3(), None).unwrap();
		assert!(jdb.exists(&foo));
		assert!(jdb.exists(&bar));

		jdb.remove(&foo);
		let baz = jdb.insert(b"baz");
		jdb.commit(1, &b"1a".sha3(), Some((0, b"0".sha3()))).unwrap();

		jdb.remove(&bar);
		jdb.commit(1, &b"1b".sha3(), Some((0, b"0".sha3()))).unwrap();

		assert!(jdb.exists(&foo));
		assert!(jdb.exists(&bar));
		assert!(jdb.exists(&baz));

		jdb.commit(2, &b"2b".sha3(), Some((1, b"1b".sha3()))).unwrap();
		assert!(jdb.exists(&foo));
		assert!(!jdb.exists(&baz));
		assert!(!jdb.exists(&bar));
	}

	#[test]
	fn overwrite() {
		// history is 1
		let mut jdb = JournalDB::new_temp();

		let foo = jdb.insert(b"foo");
		jdb.commit(0, &b"0".sha3(), None).unwrap();
		assert!(jdb.exists(&foo));

		jdb.remove(&foo);
		jdb.commit(1, &b"1".sha3(), Some((0, b"0".sha3()))).unwrap();
		jdb.insert(b"foo");
		assert!(jdb.exists(&foo));
		jdb.commit(2, &b"2".sha3(), Some((1, b"1".sha3()))).unwrap();
		assert!(jdb.exists(&foo));
		jdb.commit(3, &b"2".sha3(), Some((0, b"2".sha3()))).unwrap();
		assert!(jdb.exists(&foo));
	}

	#[test]
	fn fork_same_key() {
		// history is 1
		let mut jdb = JournalDB::new_temp();
		jdb.commit(0, &b"0".sha3(), None).unwrap();

		let foo = jdb.insert(b"foo");
		jdb.commit(1, &b"1a".sha3(), Some((0, b"0".sha3()))).unwrap();

		jdb.insert(b"foo");
		jdb.commit(1, &b"1b".sha3(), Some((0, b"0".sha3()))).unwrap();
		assert!(jdb.exists(&foo));

		jdb.commit(2, &b"2a".sha3(), Some((1, b"1a".sha3()))).unwrap();
		assert!(jdb.exists(&foo));
	}
}
