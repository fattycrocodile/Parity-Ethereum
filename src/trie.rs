extern crate rand;

use std::fmt;
use memorydb::*;
use sha3::*;
use hashdb::*;
use hash::*;
use nibbleslice::*;
use bytes::*;
use rlp::*;

//use log::*;

pub const NULL_RLP: [u8; 1] = [0x80; 1];
pub const SHA3_NULL_RLP: H256 = H256( [0x56, 0xe8, 0x1f, 0x17, 0x1b, 0xcc, 0x55, 0xa6, 0xff, 0x83, 0x45, 0xe6, 0x92, 0xc0, 0xf8, 0x6e, 0x5b, 0x48, 0xe0, 0x1b, 0x99, 0x6c, 0xad, 0xc0, 0x01, 0x62, 0x2f, 0xb5, 0xe3, 0x63, 0xb4, 0x21] );

pub trait Trie {
	fn root(&self) -> &H256;
	fn is_empty(&self) -> bool { *self.root() == SHA3_NULL_RLP }

	// TODO: consider returning &[u8]...
	fn contains(&self, key: &[u8]) -> bool;
	fn at<'a, 'key>(&'a self, key: &'key [u8]) -> Option<&'a [u8]> where 'a: 'key;
	fn insert(&mut self, key: &[u8], value: &[u8]);
	fn remove(&mut self, key: &[u8]);
}

#[derive(Eq, PartialEq, Debug)]
pub enum Node<'a> {
	Empty,
	Leaf(NibbleSlice<'a>, &'a[u8]),
	Extension(NibbleSlice<'a>, &'a[u8]),
	Branch([&'a[u8]; 16], Option<&'a [u8]>)
}

enum Operation {
	New(H256, Bytes),
	Delete(H256),
}

struct Diff (Vec<Operation>);

impl Diff {
	fn new() -> Diff { Diff(vec![]) }

	/// Given the RLP that encodes a node, append a reference to that node `out` and leave `diff`
	/// such that the reference is valid, once applied.
	fn new_node(&mut self, rlp: Bytes, out: &mut RlpStream) {
		if rlp.len() >= 32 {
			trace!("new_node: reference node {:?}", rlp.pretty());
			let rlp_sha3 = rlp.sha3();
			out.append(&rlp_sha3);
			self.0.push(Operation::New(rlp_sha3, rlp));
		}
		else {
			trace!("new_node: inline node {:?}", rlp.pretty());
			out.append_raw(&rlp, 1);
		}
	}

	/// Given the RLP that encodes a now-unused node, leave `diff` in such a state that it is noted.
	fn delete_node_sha3(&mut self, old_sha3: H256) {
		self.0.push(Operation::Delete(old_sha3));
	}

	fn delete_node(&mut self, old: &Rlp) {
		if old.is_data() && old.size() == 32 {
			self.0.push(Operation::Delete(H256::decode(old)));
		}
	}

	fn delete_node_from_slice(&mut self, old: &[u8]) {
		let r = Rlp::new(old);
		if r.is_data() && r.size() == 32 {
			self.0.push(Operation::Delete(H256::decode(&r)));
		}
	}

	fn replace_node(&mut self, old: &Rlp, rlp: Bytes, out: &mut RlpStream) {
		self.delete_node(old);
		self.new_node(rlp, out);
	}
}

impl <'a>Node<'a> {
	fn decoded(node_rlp: &'a [u8]) -> Node<'a> {
		let r = Rlp::new(node_rlp);
		match r.prototype() {
			// either leaf or extension - decode first item with NibbleSlice::??? 
			// and use is_leaf return to figure out which.
			// if leaf, second item is a value (is_data())
			// if extension, second item is a node (either SHA3 to be looked up and 
			// fed back into this function or inline RLP which can be fed back into this function).
			Prototype::List(2) => match NibbleSlice::from_encoded(r.at(0).data()) {
				(slice, true) => Node::Leaf(slice, r.at(1).data()),
				(slice, false) => Node::Extension(slice, r.at(1).raw()),
			},
			// branch - first 16 are nodes, 17th is a value (or empty).
			Prototype::List(17) => {
				let mut nodes: [&'a [u8]; 16] = unsafe { ::std::mem::uninitialized() };
				for i in 0..16 {
					nodes[i] = r.at(i).raw();
				}
				Node::Branch(nodes, if r.at(16).is_empty() { None } else { Some(r.at(16).data()) })
			},
			// an empty branch index.
			Prototype::Data(0) => Node::Empty,
			// something went wrong.
			_ => panic!("Rlp is not valid.")
		}
	}

	// todo: should check length before encoding, cause it may just be sha3 of data
	fn encoded(&self) -> Bytes {
		match *self {
			Node::Leaf(ref slice, ref value) => {
				let mut stream = RlpStream::new_list(2);
				stream.append(&slice.encoded(true));
				stream.append(value);
				stream.out()
			},
			Node::Extension(ref slice, ref raw_rlp) => {
				let mut stream = RlpStream::new_list(2);
				stream.append(&slice.encoded(false));
				stream.append_raw(raw_rlp, 1);
				stream.out()
			},
			Node::Branch(ref nodes, ref value) => {
				let mut stream = RlpStream::new_list(17);
				for i in 0..16 {
					stream.append_raw(nodes[i], 1);
				}
				match *value {
					Some(n) => { stream.append(&n); },
					None => { stream.append_empty_data(); },
				}
				stream.out()
			},
			Node::Empty => {
				let mut stream = RlpStream::new();
				stream.append_empty_data();
				stream.out()
			}
		}
	}

	fn encoded_and_added(&self, diff: &mut Diff) -> Bytes {
		let mut stream = RlpStream::new();
		match *self {
			Node::Leaf(ref slice, ref value) => {
				stream.append_list(2);
				stream.append(&slice.encoded(true));
				stream.append(value);
			},
			Node::Extension(ref slice, ref raw_rlp) => {
				stream.append_list(2);
				stream.append(&slice.encoded(false));
				stream.append_raw(raw_rlp, 1);
			},
			Node::Branch(ref nodes, ref value) => {
				stream.append_list(17);
				for i in 0..16 {
					stream.append_raw(nodes[i], 1);
				}
				match *value {
					Some(n) => { stream.append(&n); },
					None => { stream.append_empty_data(); },
				}
			},
			Node::Empty => {
				stream.append_empty_data();
			}
		}
		let node = stream.out();
		match node.len() {
			0 ... 31 => node,
			_ => {
				let mut stream = RlpStream::new();
				diff.new_node(node, &mut stream);
				stream.out()
			}
		}
	}
}

//enum ValidationResult<'a> {
	//Valid,
	//Invalid { node: Node<'a>, depth: usize }
//}

pub struct TrieDB {
	db: Box<HashDB>,
	root: H256,
}

impl fmt::Debug for TrieDB {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		try!(writeln!(f, "["));
		let root_rlp = self.db.lookup(&self.root).expect("Trie root not found!");
		try!(self.fmt_all(Node::decoded(root_rlp), f, 0));
		writeln!(f, "]")
	}
}

enum MaybeChanged<'a> {
	Same(Node<'a>),
	Changed(Bytes),
}

impl TrieDB {
	pub fn new_boxed(db_box: Box<HashDB>) -> Self { let mut r = TrieDB{ db: db_box, root: H256::new() }; r.set_root_rlp(&NULL_RLP); r }

	pub fn new<T>(db: T) -> Self where T: HashDB + 'static { Self::new_boxed(Box::new(db)) }

	pub fn new_memory() -> Self { Self::new(MemoryDB::new()) }

	pub fn db(&self) -> &HashDB { self.db.as_ref() }

	fn set_root_rlp(&mut self, root_data: &[u8]) {
		self.db.kill(&self.root);
		self.root = self.db.insert(root_data);
		trace!("set_root_rlp {:?} {:?}", root_data.pretty(), self.root);
	}

	fn apply(&mut self, diff: Diff) {
		trace!("applying {:?} changes", diff.0.len());
		for d in diff.0.into_iter() {
			match d {
				Operation::Delete(h) => {
					trace!("TrieDB::apply --- {:?}", &h);
					self.db.kill(&h);
				},
				Operation::New(h, d) => {
					trace!("TrieDB::apply +++ {:?} -> {:?}", &h, d.pretty());
					self.db.emplace(h, d);
				}
			}
		}
	}

	fn fmt_indent(&self, f: &mut fmt::Formatter, size: usize) -> fmt::Result {
		for _ in 0..size { 
			try!(write!(f, "  "));
		}
		Ok(())
	}

	fn get_node<'a>(&'a self, node: &'a [u8]) -> Node {
		Node::decoded(self.get_raw_or_lookup(node))
	}

	fn fmt_all(&self, node: Node, f: &mut fmt::Formatter, deepness: usize) -> fmt::Result {
		match node {
			Node::Leaf(slice, value) => try!(writeln!(f, "'{:?}: {:?}.", slice, value.pretty())),
			Node::Extension(ref slice, ref item) => {
				try!(write!(f, "'{:?} ", slice));
				try!(self.fmt_all(self.get_node(item), f, deepness));
			},
			Node::Branch(ref nodes, ref value) => {
				try!(writeln!(f, ""));
				match value {
					&Some(v) => {
						try!(self.fmt_indent(f, deepness + 1));
						try!(writeln!(f, "=: {:?}", v.pretty()))
					},
					&None => {}
				}
				for i in 0..16 {
					match self.get_node(nodes[i]) {
						Node::Empty => {},
						n => {
							try!(self.fmt_indent(f, deepness + 1));
							try!(write!(f, "'{:x} ", i));
							try!(self.fmt_all(n, f, deepness + 1));
						}
					}
				}
			},
			// empty
			Node::Empty => {
				try!(writeln!(f, "<empty>"));
			}
		};
		Ok(())
	}

	fn get<'a, 'key>(&'a self, key: &NibbleSlice<'key>) -> Option<&'a [u8]> where 'a: 'key {
		let root_rlp = self.db.lookup(&self.root).expect("Trie root not found!");
		self.get_from_node(&root_rlp, key)
	}

	fn get_from_node<'a, 'key>(&'a self, node: &'a [u8], key: &NibbleSlice<'key>) -> Option<&'a [u8]> where 'a: 'key {
		match Node::decoded(node) {
			Node::Leaf(ref slice, ref value) if key == slice => Some(value),
			Node::Extension(ref slice, ref item) if key.starts_with(slice) => {
				self.get_from_node(self.get_raw_or_lookup(item), &key.mid(slice.len()))
			},
			Node::Branch(ref nodes, value) => match key.is_empty() {
				true => value,
				false => self.get_from_node(self.get_raw_or_lookup(nodes[key.at(0) as usize]), &key.mid(1))
			},
			_ => None
		}
	}

	fn get_raw_or_lookup<'a>(&'a self, node: &'a [u8]) -> &'a [u8] {
		// check if its sha3 + len
		let r = Rlp::new(node);
		match r.is_data() && r.size() == 32 {
			true => self.db.lookup(&H256::decode(&r)).expect("Not found!"),
			false => node
		}
	}

	fn add(&mut self, key: &NibbleSlice, value: &[u8]) {
		trace!("ADD: {:?} {:?}", key, value.pretty());
		// determine what the new root is, insert new nodes and remove old as necessary.
		let mut todo: Diff = Diff::new();
		let root_rlp = self.augmented(self.db.lookup(&self.root).expect("Trie root not found!"), key, value, &mut todo);
		self.apply(todo);
		self.set_root_rlp(&root_rlp);
		trace!("/");
	}

	fn delete(&mut self, key: &NibbleSlice) {
		trace!("DELETE: {:?}", key);
		// determine what the new root is, insert new nodes and remove old as necessary.
		let mut todo: Diff = Diff::new();
		match self.cleared_from_slice(self.db.lookup(&self.root).expect("Trie root not found!"), key, &mut todo) {
			Some(root_rlp) => {
				self.apply(todo);
				self.set_root_rlp(&root_rlp);
			},
			None => {
				trace!("no change needed");
			}
		}
		trace!("/");
	}

	fn compose_leaf(partial: &NibbleSlice, value: &[u8]) -> Bytes {
		trace!("compose_leaf {:?} {:?} ({:?})", partial, value.pretty(), partial.encoded(true).pretty());
		let mut s = RlpStream::new_list(2);
		s.append(&partial.encoded(true));
		s.append(&value);
		let r = s.out();
		trace!("compose_leaf: -> {:?}", r.pretty());
		r
	}

	fn compose_raw(partial: &NibbleSlice, raw_payload: &[u8], is_leaf: bool) -> Bytes {
		trace!("compose_raw {:?} {:?} {:?} ({:?})", partial, raw_payload.pretty(), is_leaf, partial.encoded(is_leaf));
		let mut s = RlpStream::new_list(2);
		s.append(&partial.encoded(is_leaf));
		s.append_raw(raw_payload, 1);
		let r = s.out();
		trace!("compose_raw: -> {:?}", r.pretty());
		r
	}

	fn compose_stub_branch(value: &[u8]) -> Bytes {
		let mut s = RlpStream::new_list(17);
		for _ in 0..16 { s.append_empty_data(); }
		s.append(&value);
		s.out()
	}

	fn compose_extension(partial: &NibbleSlice, raw_payload: &[u8]) -> Bytes {
		Self::compose_raw(partial, raw_payload, false)
	}

	fn create_extension(partial: &NibbleSlice, downstream_node: Bytes, diff: &mut Diff) -> Bytes {
		trace!("create_extension partial: {:?}, downstream_node: {:?}", partial, downstream_node.pretty());
		let mut s = RlpStream::new_list(2);
		s.append(&partial.encoded(false));
		diff.new_node(downstream_node, &mut s);
		s.out()
	}

	/// Return the bytes encoding the node represented by `rlp`. It will be unlinked from
	/// the trie.
	fn take_node<'a, 'rlp_view>(&'a self, rlp: &'rlp_view Rlp<'a>, diff: &mut Diff) -> &'a [u8] where 'a: 'rlp_view {
		if rlp.is_list() {
			trace!("take_node {:?} (inline)", rlp.raw().pretty());
			rlp.raw()
		}
		else if rlp.is_data() && rlp.size() == 32 {
			let h = H256::decode(rlp);
			let r = self.db.lookup(&h).expect("Trie root not found!");
			trace!("take_node {:?} (indirect for {:?})", rlp.raw().pretty(), r);
			diff.delete_node_sha3(h);
			r
		}
		else {
			trace!("take_node {:?} (???)", rlp.raw().pretty());
			panic!("Empty or invalid node given?");
		}
	}

	/// Transform an existing extension or leaf node to an invalid single-entry branch.
	///
	/// **This operation will not insert the new node nor destroy the original.**
	fn transmuted_extension_to_branch(orig_partial: &NibbleSlice, orig_raw_payload: &[u8], diff: &mut Diff) -> Bytes {
		trace!("transmuted_extension_to_branch");
		let mut s = RlpStream::new_list(17);
		assert!(!orig_partial.is_empty());	// extension nodes are not allowed to have empty partial keys.
		let index = orig_partial.at(0);
		// orig is extension - orig_raw_payload is a node itself.
		for i in 0..17 {
			if index == i {
				if orig_partial.len() > 1 {
					// still need an extension
					diff.new_node(Self::compose_extension(&orig_partial.mid(1), orig_raw_payload), &mut s);
				} else {
					// was an extension of length 1 - just redirect the payload into here.
					s.append_raw(orig_raw_payload, 1);
				}
			} else {
				s.append_empty_data();
			}
		}
		s.out()
	}

	fn transmuted_leaf_to_branch(orig_partial: &NibbleSlice, value: &[u8], diff: &mut Diff) -> Bytes {
		trace!("transmuted_leaf_to_branch");
		let mut s = RlpStream::new_list(17);
		let index = if orig_partial.is_empty() {16} else {orig_partial.at(0)};
		// orig is leaf - orig_raw_payload is data representing the actual value.
		for i in 0..17 {
			match (index == i, i) {
				(true, 16) => // leaf entry - just replace.
					{ s.append(&value); },
				(true, _) => // easy - original had empty slot.
					diff.new_node(Self::compose_leaf(&orig_partial.mid(1), value), &mut s),
				(false, _) => { s.append_empty_data(); }
			}
		}
		s.out()
	}

	/// Transform an existing extension or leaf node plus a new partial/value to a two-entry branch.
	///
	/// **This operation will not insert the new node nor destroy the original.**
	fn transmuted_to_branch_and_augmented(&self, orig_is_leaf: bool, orig_partial: &NibbleSlice, orig_raw_payload: &[u8], partial: &NibbleSlice, value: &[u8], diff: &mut Diff) -> Bytes {
		trace!("transmuted_to_branch_and_augmented");
		let intermediate = match orig_is_leaf {
			true => Self::transmuted_leaf_to_branch(orig_partial, Rlp::new(orig_raw_payload).data(), diff),
			false => Self::transmuted_extension_to_branch(orig_partial, orig_raw_payload, diff),
		};
		self.augmented(&intermediate, partial, value, diff)
		// TODO: implement without having to make an intermediate representation.
	}

	/// Given a branch node's RLP `orig` together with a `partial` key and `value`, return the
	/// RLP-encoded node that accomodates the trie with the new entry. Mutate `diff` so that
	/// once applied the returned node is valid.
	fn augmented_into_branch(&self, orig: &Rlp, partial: &NibbleSlice, value: &[u8], diff: &mut Diff) -> Bytes {
		trace!("augmented_into_branch");
		let mut s = RlpStream::new_list(17);
		let index = if partial.is_empty() {16} else {partial.at(0) as usize};
		for i in 0usize..17 {
			match (index == i, i) {
				(true, 16) => // leaf entry - just replace.
					{ s.append(&value); },
				(true, i) if orig.at(i).is_empty() => // easy - original had empty slot.
					diff.new_node(Self::compose_leaf(&partial.mid(1), value), &mut s),
				(true, i) => {	// harder - original has something there already
					let new = self.augmented(self.take_node(&orig.at(i), diff), &partial.mid(1), value, diff);
					diff.replace_node(&orig.at(i), new, &mut s);
				}
				(false, i) => { s.append_raw(orig.at(i).raw(), 1); },
			}
		}
		s.out()
	}

	/// Determine the RLP of the node, assuming we're inserting `partial` into the
	/// node currently of data `old`. This will *not* delete any hash of `old` from the database;
	/// it will just return the new RLP that includes the new node.
	///
	/// The database will be updated so as to make the returned RLP valid through inserting
	/// and deleting nodes as necessary.
	///
	/// **This operation will not insert the new node nor destroy the original.**
	fn augmented(&self, old: &[u8], partial: &NibbleSlice, value: &[u8], diff: &mut Diff) -> Bytes {
		trace!("augmented (old: {:?}, partial: {:?}, value: {:?})", old.pretty(), partial, value.pretty());
		// already have an extension. either fast_forward, cleve or transmute_to_branch.
		let old_rlp = Rlp::new(old);
		match old_rlp.prototype() {
			Prototype::List(17) => {
				trace!("branch: ROUTE,AUGMENT");
				// already have a branch. route and augment.
				self.augmented_into_branch(&old_rlp, partial, value, diff)
			},
			Prototype::List(2) => {
				let existing_key_rlp = old_rlp.at(0);
				let (existing_key, is_leaf) = NibbleSlice::from_encoded(existing_key_rlp.data());
				match (is_leaf, partial.common_prefix(&existing_key)) {
					(true, cp) if cp == existing_key.len() && partial.len() == existing_key.len() => {
						// equivalent-leaf: replace
						trace!("equivalent-leaf: REPLACE");
						Self::compose_leaf(partial, value)
					},
					(_, 0) => {
						// one of us isn't empty: transmute to branch here
						trace!("no-common-prefix, not-both-empty (exist={:?}; new={:?}): TRANSMUTE,AUGMENT", existing_key.len(), partial.len());
						self.transmuted_to_branch_and_augmented(is_leaf, &existing_key, old_rlp.at(1).raw(), partial, value, diff)
					},
					(_, cp) if cp == existing_key.len() => {
						trace!("complete-prefix (cp={:?}): AUGMENT-AT-END", cp);
						// fully-shared prefix for this extension:
						// transform to an extension + augmented version of onward node.
						let downstream_node: Bytes = if is_leaf {
							// no onward node because we're a leaf - create fake stub and use that.
							self.augmented(&Self::compose_stub_branch(old_rlp.at(1).data()), &partial.mid(cp), value, diff)
						} else {
							self.augmented(self.take_node(&old_rlp.at(1), diff), &partial.mid(cp), value, diff)
						};
						Self::create_extension(&existing_key, downstream_node, diff)
					},
					(_, cp) => {
						// partially-shared prefix for this extension:
						// split into two extensions, high and low, pass the
						// low through augment with the value before inserting the result
						// into high to create the new.

						// TODO: optimise by doing this without creating augmented_low.

						trace!("partially-shared-prefix (exist={:?}; new={:?}; cp={:?}): AUGMENT-AT-END", existing_key.len(), partial.len(), cp);

						// low (farther from root)
						let low = Self::compose_raw(&existing_key.mid(cp), old_rlp.at(1).raw(), is_leaf);
						let augmented_low = self.augmented(&low, &partial.mid(cp), value, diff);

						// high (closer to root)
						let mut s = RlpStream::new_list(2);
						s.append(&existing_key.encoded_leftmost(cp, false));
						diff.new_node(augmented_low, &mut s);
						s.out()
					},
				}
			},
			Prototype::Data(0) => {
				trace!("empty: COMPOSE");
				Self::compose_leaf(partial, value)
			},
			_ => panic!("Invalid RLP for node: {:?}", old.pretty()),
		}
	}

	fn encoded(n: MaybeChanged) -> Bytes {
		match n {
			MaybeChanged::Same(n) => n.encoded(),
			MaybeChanged::Changed(b) => b,
		}
	}

	fn fixed_indirection<'a>(n: Node<'a>, diff: &mut Diff) -> MaybeChanged<'a> {
		match n {
			Node::Extension(partial, payload) if payload.len() >= 32 && Rlp::new(payload).is_list() => {
				// make indirect
				MaybeChanged::Changed(Node::Extension(partial, &Node::decoded(payload).encoded_and_added(diff)).encoded())
			},
			Node::Branch(payloads, value) => {
				// check each child isn't too big
				// TODO OPTIMISE - should really check at the point of (re-)constructing the branch.
				for i in 0..16 {
					if payloads[i].len() >= 32 && Rlp::new(payloads[i]).is_list() {
						let n = Node::decoded(payloads[i]).encoded_and_added(diff);
						let mut new_nodes = payloads;
						new_nodes[i] = &n;
						return MaybeChanged::Changed(Node::Branch(new_nodes, value).encoded())
					}
				}
				MaybeChanged::Same(n)
			}
			_ => MaybeChanged::Same(n),
		}
	}

	/// Given a node `n` which may be in an _invalid state_, fix it such that it is then in a valid
	/// state.
	///
	/// _invalid state_ means:
	/// - Branch node where there is only a single entry;
	/// - Extension node followed by anything other than a Branch node.
	/// - Extension node with a child which has too many bytes to be inline.
	///
	/// **This operation will not insert the new node nor destroy the original.**
	fn fixed<'a, 'b>(&'a self, n: Node<'b>, diff: &mut Diff) -> MaybeChanged<'b> where 'a: 'b {
		trace!("fixed node={:?}", n);
		match n {
			Node::Branch(nodes, node_value) => {
				// if only a single value, transmute to leaf/extension and feed through fixed.
				#[derive(Debug)]
				enum UsedIndex {
					None,
					One(u8),
					Many,
				};
				let mut used_index = UsedIndex::None;
				// 0-15 -> index of a non-null branch
				// 16 -> no non-null branch
				// 17 -> multiple non-null branches
				for i in 0..16 {
					match (nodes[i] == NULL_RLP, &used_index) {
						(false, &UsedIndex::None) => used_index = UsedIndex::One(i as u8),
						(false, &UsedIndex::One(_)) => used_index = UsedIndex::Many,
						(_, _) => {},
					}
				}
				trace!("branch: used_index={:?}, node_value={:?}", used_index, node_value);
				match (used_index, node_value) {
					(UsedIndex::None, None) => panic!("Branch with no subvalues. Something went wrong."),
					(UsedIndex::One(a), None) => {		// one onward node
						// transmute to extension.
						// TODO: OPTIMISE: - don't call fixed again but put the right node in straight away here.
						// call fixed again since the transmute may cause invalidity.
						let new_partial: [u8; 1] = [a; 1];
						MaybeChanged::Changed(Self::encoded(self.fixed(Node::Extension(NibbleSlice::new_offset(&new_partial[..], 1), nodes[a as usize]), diff)))
					},
					(UsedIndex::None, Some(value)) => {		// one leaf value
						// transmute to leaf.
						// call fixed again since the transmute may cause invalidity.
						MaybeChanged::Changed(Self::encoded(self.fixed(Node::Leaf(NibbleSlice::new(&b""[..]), value), diff)))
					}
					_ => {						// onwards node(s) and/or leaf
						// no transmute needed, but should still fix the indirection.
						trace!("no-transmute: FIXINDIRECTION");
						Self::fixed_indirection(Node::Branch(nodes, node_value), diff)
					},
				}
			},
			Node::Extension(partial, payload) => {
				match Node::decoded(self.get_raw_or_lookup(payload)) {
					Node::Extension(sub_partial, sub_payload) => {
						// combine with node below
						diff.delete_node_from_slice(payload);
						MaybeChanged::Changed(Self::encoded(Self::fixed_indirection(Node::Extension(NibbleSlice::new_composed(&partial, &sub_partial), sub_payload), diff)))
					},
					Node::Leaf(sub_partial, sub_value) => {
						// combine with node below
						diff.delete_node_from_slice(payload);
						MaybeChanged::Changed(Self::encoded(Self::fixed_indirection(Node::Leaf(NibbleSlice::new_composed(&partial, &sub_partial), sub_value), diff)))
					},
					// no change, might still have an oversize node inline - fix indirection
					_ => Self::fixed_indirection(n, diff),
				}
			},
			// leaf or empty. no change.
			n => { MaybeChanged::Same(n) }
		}
	}

	/// Determine the RLP of the node, assuming we're removing `partial` from the
	/// node currently of data `old`. This will *not* delete any hash of `old` from the database;
	/// it will just return the new RLP that represents the new node.
	/// `None` may be returned should no change be needed.
	///
	/// The database will be updated so as to make the returned RLP valid through inserting
	/// and deleting nodes as necessary.
	///
	/// **This operation will not insert the new node nor destroy the original.**
	fn cleared_from_slice(&self, old: &[u8], partial: &NibbleSlice, diff: &mut Diff) -> Option<Bytes> {
		self.cleared(Node::decoded(old), partial, diff)
	}

	fn cleared(&self, n: Node, partial: &NibbleSlice, diff: &mut Diff) -> Option<Bytes> {
		trace!("cleared old={:?}, partial={:?})", n, partial);

		match (n, partial.is_empty()) {
			(Node::Empty, _) => None,
			(Node::Branch(_, None), true) => { None },
			(Node::Branch(payloads, _), true) => Some(Self::encoded(self.fixed(Node::Branch(payloads, None), diff))),	// matched as leaf-branch - give back fixed branch with it.
			(Node::Branch(payloads, value), false) => {
				// Branch with partial left - route, clear, fix.
				let i: usize = partial.at(0) as usize;
				trace!("branch-with-partial node[{:?}]={:?}", i, payloads[i].pretty());
				self.cleared(self.get_node(payloads[i]), &partial.mid(1), diff).map(|new_payload| {
					trace!("branch-new-payload={:?}; delete-old={:?}", new_payload.pretty(), payloads[i].pretty());

					// downsteam node needed to be changed.
					diff.delete_node_from_slice(payloads[i]);
					// return fixed up new node.
					let mut new_payloads = payloads;
					new_payloads[i] = &new_payload;
					Self::encoded(self.fixed(Node::Branch(new_payloads, value), diff))
				})
			},
			(Node::Leaf(node_partial, _), _) => {
				trace!("leaf partial={:?}", node_partial);
				match node_partial.common_prefix(partial) {
					cp if cp == partial.len() => {		// leaf to be deleted - delete it :)
						trace!("matched-prefix (cp={:?}): REPLACE-EMPTY", cp);
						Some(Node::Empty.encoded())
					},
					_ => None,												// anything else and the key doesn't exit - no change.
				}
			},
			(Node::Extension(node_partial, node_payload), _) => {
				trace!("extension partial={:?}, payload={:?}", node_partial, node_payload.pretty());
				match node_partial.common_prefix(partial) {
					cp if cp == node_partial.len() => {
						trace!("matching-prefix (cp={:?}): SKIP,CLEAR,FIXUP", cp);
						// key at end of extension - skip, clear, fix
						self.cleared(self.get_node(node_payload), &partial.mid(node_partial.len()), diff).map(|new_payload| {
							trace!("extension-new-payload={:?}; delete-old={:?}", new_payload.pretty(), node_payload.pretty());
							// downsteam node needed to be changed.
							diff.delete_node_from_slice(node_payload);
							// return fixed up new node.
							Self::encoded(self.fixed(Node::Extension(node_partial, &new_payload), diff))
						})
					},
					_ => None,	// key in the middle of an extension - doesn't exist.
				}
			},
		}
	}
}

impl Trie for TrieDB {
	fn root(&self) -> &H256 { &self.root }

	fn contains(&self, key: &[u8]) -> bool {
		self.at(key).is_some()
	}

	fn at<'a, 'key>(&'a self, key: &'key [u8]) -> Option<&'a [u8]> where 'a: 'key {
		self.get(&NibbleSlice::new(key))
	}

	fn insert(&mut self, key: &[u8], value: &[u8]) {
		self.add(&NibbleSlice::new(key), value);
	}

	fn remove(&mut self, key: &[u8]) {
		self.delete(&NibbleSlice::new(key));
	}
}

#[cfg(test)]
mod tests {
	extern crate json_tests;
	use self::json_tests::{trie, execute_tests_from_directory};
	use triehash::*;
	use hash::*;
	use super::*;
	use nibbleslice::*;
	use rlp;
	use env_logger;
	use rand::random;
	use bytes::ToPretty;

	#[test]
	fn playpen() {
		env_logger::init().ok();

		let big_value = b"00000000000000000000000000000000";

		let mut t1 = TrieDB::new_memory();
		t1.insert(&[0x01, 0x23], &big_value.to_vec());
		t1.insert(&[0x01, 0x34], &big_value.to_vec());
		let mut t2 = TrieDB::new_memory();
		t2.insert(&[0x01], &big_value.to_vec());
		t2.insert(&[0x01, 0x23], &big_value.to_vec());
		t2.insert(&[0x01, 0x34], &big_value.to_vec());
		t2.remove(&[0x01]);
		/*if t1.root() != t2.root()*/ {
			trace!("{:?}", t1);
			trace!("{:?}", t2);
		}
	}

	#[test]
	fn init() {
		let t = TrieDB::new_memory();
		assert_eq!(*t.root(), SHA3_NULL_RLP);
		assert!(t.is_empty());
	}

	#[test]
	fn insert_on_empty() {
		let mut t = TrieDB::new_memory();
		t.insert(&[0x01u8, 0x23], &[0x01u8, 0x23]);
		assert_eq!(*t.root(), trie_root(vec![ (vec![0x01u8, 0x23], vec![0x01u8, 0x23]) ]));
	}

	#[test]
	fn remove_to_empty() {
	}

	#[test]
	fn insert_replace_root() {
		let mut t = TrieDB::new_memory();
		t.insert(&[0x01u8, 0x23], &[0x01u8, 0x23]);
		t.insert(&[0x01u8, 0x23], &[0x23u8, 0x45]);
		assert_eq!(*t.root(), trie_root(vec![ (vec![0x01u8, 0x23], vec![0x23u8, 0x45]) ]));
	}

	#[test]
	fn insert_make_branch_root() {
		let mut t = TrieDB::new_memory();
		t.insert(&[0x01u8, 0x23], &[0x01u8, 0x23]);
		t.insert(&[0x11u8, 0x23], &[0x11u8, 0x23]);
		assert_eq!(*t.root(), trie_root(vec![
			(vec![0x01u8, 0x23], vec![0x01u8, 0x23]),
			(vec![0x11u8, 0x23], vec![0x11u8, 0x23])
		]));
	}

	#[test]
	fn insert_into_branch_root() {
		let mut t = TrieDB::new_memory();
		t.insert(&[0x01u8, 0x23], &[0x01u8, 0x23]);
		t.insert(&[0xf1u8, 0x23], &[0xf1u8, 0x23]);
		t.insert(&[0x81u8, 0x23], &[0x81u8, 0x23]);
		assert_eq!(*t.root(), trie_root(vec![
			(vec![0x01u8, 0x23], vec![0x01u8, 0x23]),
			(vec![0x81u8, 0x23], vec![0x81u8, 0x23]),
			(vec![0xf1u8, 0x23], vec![0xf1u8, 0x23]),
		]));
	}

	#[test]
	fn insert_value_into_branch_root() {
		let mut t = TrieDB::new_memory();
		t.insert(&[0x01u8, 0x23], &[0x01u8, 0x23]);
		t.insert(&[], &[0x0]);
		assert_eq!(*t.root(), trie_root(vec![
			(vec![], vec![0x0]),
			(vec![0x01u8, 0x23], vec![0x01u8, 0x23]),
		]));
	}

	#[test]
	fn insert_split_leaf() {
		let mut t = TrieDB::new_memory();
		t.insert(&[0x01u8, 0x23], &[0x01u8, 0x23]);
		t.insert(&[0x01u8, 0x34], &[0x01u8, 0x34]);
		assert_eq!(*t.root(), trie_root(vec![
			(vec![0x01u8, 0x23], vec![0x01u8, 0x23]),
			(vec![0x01u8, 0x34], vec![0x01u8, 0x34]),
		]));
	}

	#[test]
	fn insert_split_extenstion() {
		let mut t = TrieDB::new_memory();
		t.insert(&[0x01, 0x23, 0x45], &[0x01]);
		t.insert(&[0x01, 0xf3, 0x45], &[0x02]);
		t.insert(&[0x01, 0xf3, 0xf5], &[0x03]);
		assert_eq!(*t.root(), trie_root(vec![
			(vec![0x01, 0x23, 0x45], vec![0x01]),
			(vec![0x01, 0xf3, 0x45], vec![0x02]),
			(vec![0x01, 0xf3, 0xf5], vec![0x03]),
		]));
	}

	#[test]
	fn insert_big_value() {
		let big_value0 = b"00000000000000000000000000000000";
		let big_value1 = b"11111111111111111111111111111111";

		let mut t = TrieDB::new_memory();
		t.insert(&[0x01u8, 0x23], big_value0);
		t.insert(&[0x11u8, 0x23], big_value1);
		assert_eq!(*t.root(), trie_root(vec![
			(vec![0x01u8, 0x23], big_value0.to_vec()),
			(vec![0x11u8, 0x23], big_value1.to_vec())
		]));
	}

	#[test]
	fn insert_duplicate_value() {
		let big_value = b"00000000000000000000000000000000";

		let mut t = TrieDB::new_memory();
		t.insert(&[0x01u8, 0x23], big_value);
		t.insert(&[0x11u8, 0x23], big_value);
		assert_eq!(*t.root(), trie_root(vec![
			(vec![0x01u8, 0x23], big_value.to_vec()),
			(vec![0x11u8, 0x23], big_value.to_vec())
		]));
	}

	#[test]
	fn test_node_leaf() {
		let k = vec![0x20u8, 0x01, 0x23, 0x45];
		let v: Vec<u8> = From::from("cat");
		let (slice, is_leaf) = NibbleSlice::from_encoded(&k);
		assert_eq!(is_leaf, true);
		let leaf = Node::Leaf(slice, &v);
		let rlp = leaf.encoded();
		let leaf2 = Node::decoded(&rlp);
		assert_eq!(leaf, leaf2);
	}

	#[test]
	fn test_node_extension() {
		let k = vec![0x00u8, 0x01, 0x23, 0x45];
		// in extension, value must be valid rlp
		let v = rlp::encode(&"cat");
		let (slice, is_leaf) = NibbleSlice::from_encoded(&k);
		assert_eq!(is_leaf, false);
		let ex = Node::Extension(slice, &v);
		let rlp = ex.encoded();
		let ex2 = Node::decoded(&rlp);
		assert_eq!(ex, ex2);
	}

	#[test]
	fn test_node_empty_branch() {
		let null_rlp = NULL_RLP;
		let branch = Node::Branch([&null_rlp; 16], None);
		let rlp = branch.encoded();
		let branch2 = Node::decoded(&rlp);
		println!("{:?}", rlp);
		assert_eq!(branch, branch2);
	}

	#[test]
	fn test_node_branch() {
		let k = rlp::encode(&"cat");
		let mut nodes: [&[u8]; 16] = unsafe { ::std::mem::uninitialized() };
		for i in 0..16 { nodes[i] = &k; }
		let v: Vec<u8> = From::from("dog");
		let branch = Node::Branch(nodes, Some(&v));
		let rlp = branch.encoded();
		let branch2 = Node::decoded(&rlp);
		assert_eq!(branch, branch2);
	}

	#[test]
	fn test_at_empty() {
		let t = TrieDB::new_memory();
		assert_eq!(t.at(&[0x5]), None);
	}

	#[test]
	fn test_at_one() {
		let mut t = TrieDB::new_memory();
		t.insert(&[0x01u8, 0x23], &[0x01u8, 0x23]);
		assert_eq!(t.at(&[0x1, 0x23]).unwrap(), &[0x1u8, 0x23]);
	}

	#[test]
	fn test_at_three() {
		let mut t = TrieDB::new_memory();
		t.insert(&[0x01u8, 0x23], &[0x01u8, 0x23]);
		t.insert(&[0xf1u8, 0x23], &[0xf1u8, 0x23]);
		t.insert(&[0x81u8, 0x23], &[0x81u8, 0x23]);
		assert_eq!(t.at(&[0x01, 0x23]).unwrap(), &[0x01u8, 0x23]);
		assert_eq!(t.at(&[0xf1, 0x23]).unwrap(), &[0xf1u8, 0x23]);
		assert_eq!(t.at(&[0x81, 0x23]).unwrap(), &[0x81u8, 0x23]);
		assert_eq!(t.at(&[0x82, 0x23]), None);
	}

	#[test]
	fn test_print_trie() {
		let mut t = TrieDB::new_memory();
		t.insert(&[0x01u8, 0x23], &[0x01u8, 0x23]);
		t.insert(&[0x02u8, 0x23], &[0x01u8, 0x23]);
		t.insert(&[0xf1u8, 0x23], &[0xf1u8, 0x23]);
		t.insert(&[0x81u8, 0x23], &[0x81u8, 0x23]);
		println!("trie:");
		println!("{:?}", t);
		//assert!(false);
	}

	fn random_key() -> Vec<u8> {
		let chars = b"abcdefgrstuvwABCDEFGRSTUVW";
		let mut ret: Vec<u8> = Vec::new();
		let r = random::<u8>() % 4 + 1;
		for _ in 0..r {
			ret.push(chars[random::<usize>() % chars.len()]);
		}
		ret
	}

	#[test]
	fn stress() {
		for _ in 0..5000 {
			let mut x: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
			for j in 0..4u32 {
				let key = random_key();
				x.push((key, rlp::encode(&j)));
			}
			let real = trie_root(x.clone());
			let memtrie = trie_root_mem(&x);
			let mut y = x.clone();
			y.sort_by(|ref a, ref b| a.0.cmp(&b.0));
			let memtrie_sorted = trie_root_mem(&y);
			if *memtrie.root() != real || *memtrie_sorted.root() != real {
				println!("TRIE MISMATCH");
				println!("");
				println!("ORIGINAL... {:?}", memtrie.root());
				for i in x.iter() {
					println!("{:?} -> {:?}", i.0.pretty(), i.1.pretty());
				}
				println!("{:?}", memtrie);
				println!("SORTED... {:?}", memtrie_sorted.root());
				for i in y.iter() {
					println!("{:?} -> {:?}", i.0.pretty(), i.1.pretty());
				}
				println!("{:?}", memtrie_sorted);
			}
			assert_eq!(*memtrie.root(), real);
			assert_eq!(*memtrie_sorted.root(), real);
		}
	}

	fn trie_root_mem(v: &Vec<(Vec<u8>, Vec<u8>)>) -> TrieDB {
		let mut t = TrieDB::new_memory();
		
		for i in 0..v.len() {
			let key: &[u8]= &v[i].0;
			let val: &[u8] = &v[i].1;
			t.insert(&key, &val);
		}

		t
	}

	#[test]
	fn test_trie_json() {
		println!("Json trie test: ");
		execute_tests_from_directory::<trie::TrieTest, _>("json-tests/json/trie/*.json", &mut | file, input, output | {
			println!("file: {}", file);

			let mut t = TrieDB::new_memory();
			for operation in input.into_iter() {
				match operation {
					trie::Operation::Insert(key, value) => t.insert(&key, &value),
					trie::Operation::Remove(key) => t.remove(&key)
				}
			}

			assert_eq!(*t.root(), H256::from_slice(&output));
		});
	}
}
