//! Key-value datastore with a modified Merkle tree.
extern crate rand;

use bytes::*;
use sha3::*;
use hash::*;

/// Alphabet to use when creating words for insertion into tries.
pub enum Alphabet {
	/// TODO [Gav Wood] Please document me
	All,
	/// TODO [Gav Wood] Please document me
	Low,
	/// TODO [Gav Wood] Please document me
	Mid,
	/// TODO [Gav Wood] Please document me
	Custom(Bytes),
}

/// Means of determining the value.
pub enum ValueMode {
	/// Same as the key.
	Mirror,
	/// Randomly (50:50) 1 or 32 byte randomly string.
	Random,
}

/// Standard test map for profiling tries.
pub struct StandardMap {
	/// The alphabet to use for keys.
	pub alphabet: Alphabet,
	/// Minimum size of key.
	pub min_key: usize,
	/// Delta size of key.
	pub journal_key: usize,
	/// Mode of value generation.
	pub value_mode: ValueMode,
	/// Number of keys.
	pub count: usize,
}

impl StandardMap {
	/// Get a bunch of random bytes, at least `min_count` bytes, at most `min_count` + `journal_count` bytes.
	/// `seed` is mutated pseudoramdonly and used.
	fn random_bytes(min_count: usize, journal_count: usize, seed: &mut H256) -> Vec<u8> {
		assert!(min_count + journal_count <= 32);
		*seed = seed.sha3();
		let r = min_count + (seed.bytes()[31] as usize % (journal_count + 1));
		seed.bytes()[0..r].to_vec()
	}

	/// Get a random value. Equal chance of being 1 byte as of 32. `seed` is mutated pseudoramdonly and used.
	fn random_value(seed: &mut H256) -> Bytes {
		*seed = seed.sha3();
		match seed.bytes()[0] % 2 {
			1 => vec![seed.bytes()[31];1],
			_ => seed.bytes().to_vec(),
		}
	}

	/// Get a random word of, at least `min_count` bytes, at most `min_count` + `journal_count` bytes.
	/// Each byte is an item from `alphabet`. `seed` is mutated pseudoramdonly and used.
	fn random_word(alphabet: &[u8], min_count: usize, journal_count: usize, seed: &mut H256) -> Vec<u8> {
		assert!(min_count + journal_count <= 32);
		*seed = seed.sha3();
		let r = min_count + (seed.bytes()[31] as usize % (journal_count + 1));
		let mut ret: Vec<u8> = Vec::with_capacity(r);
		for i in 0..r {
			ret.push(alphabet[seed.bytes()[i] as usize % alphabet.len()]);
		}
		ret
	}

	/// Create the standard map (set of keys and values) for the object's fields.
	pub fn make(&self) -> Vec<(Bytes, Bytes)> {
		let low = b"abcdef";
		let mid = b"@QWERTYUIOPASDFGHJKLZXCVBNM[/]^_";

		let mut d: Vec<(Bytes, Bytes)> = Vec::new();
		let mut seed = H256::new();
		for _ in 0..self.count {
			let k = match self.alphabet {
				Alphabet::All => Self::random_bytes(self.min_key, self.journal_key, &mut seed),
				Alphabet::Low => Self::random_word(low, self.min_key, self.journal_key, &mut seed),
				Alphabet::Mid => Self::random_word(mid, self.min_key, self.journal_key, &mut seed),
				Alphabet::Custom(ref a) => Self::random_word(&a, self.min_key, self.journal_key, &mut seed),
			};
			let v = match self.value_mode { ValueMode::Mirror => k.clone(), ValueMode::Random => Self::random_value(&mut seed) };
			d.push((k, v))
		}
		d
	}
}
