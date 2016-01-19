#![allow(ptr_arg)] // Because of &LastHashes -> &Vec<_>

use common::*;
use engine::*;
use state::*;
use verification::PreVerifiedBlock;

/// A transaction/receipt execution entry.
pub struct Entry {
	transaction: Transaction,
	receipt: Receipt,
}

/// Internal type for a block's common elements.
pub struct Block {
	header: Header,

	/// State is the most final state in the block.
	state: State,

	archive: Vec<Entry>,
	archive_set: HashSet<H256>,

	uncles: Vec<Header>,
}

/// A set of references to `Block` fields that are publicly accessible. 
pub struct BlockRefMut<'a> {
	pub header: &'a Header,
	pub state: &'a mut State,
	pub archive: &'a Vec<Entry>,
	pub uncles: &'a Vec<Header>,
}

impl Block {
	/// Create a new block from the given `state`.
	fn new(state: State) -> Block {
		Block {
			header: Header::new(),
			state: state,
			archive: Vec::new(),
			archive_set: HashSet::new(),
			uncles: Vec::new(),
		}
	}

	/// Get a structure containing individual references to all public fields.
	pub fn fields(&mut self) -> BlockRefMut {
		BlockRefMut {
			header: &self.header,
			state: &mut self.state,
			archive: &self.archive,
			uncles: &self.uncles,
		}
	}
}

/// Trait for a object that is_a `Block`.
pub trait IsBlock {
	/// Get the block associated with this object.
	fn block(&self) -> &Block;

	/// Get the header associated with this object's block.
	fn header(&self) -> &Header { &self.block().header }

	/// Get the final state associated with this object's block.
	fn state(&self) -> &State { &self.block().state }

	/// Get all information on transactions in this block.
	fn archive(&self) -> &Vec<Entry> { &self.block().archive }

	/// Get all uncles in this block.
	fn uncles(&self) -> &Vec<Header> { &self.block().uncles }
}

impl IsBlock for Block {
	fn block(&self) -> &Block { self }
}

/// Block that is ready for transactions to be added.
///
/// It's a bit like a Vec<Transaction>, eccept that whenever a transaction is pushed, we execute it and
/// maintain the system `state()`. We also archive execution receipts in preparation for later block creation.
pub struct OpenBlock<'x, 'y> {
	block: Block,
	engine: &'x Engine,
	last_hashes: &'y LastHashes,
}

/// Just like OpenBlock, except that we've applied `Engine::on_close_block`, finished up the non-seal header fields,
/// and collected the uncles.
///
/// There is no function available to push a transaction. If you want that you'll need to `reopen()` it.
pub struct ClosedBlock<'x, 'y> {
	open_block: OpenBlock<'x, 'y>,
	uncle_bytes: Bytes,
}

/// A block that has a valid seal.
///
/// The block's header has valid seal arguments. The block cannot be reversed into a ClosedBlock or OpenBlock.
pub struct SealedBlock {
	block: Block,
	uncle_bytes: Bytes,
}

impl<'x, 'y> OpenBlock<'x, 'y> {
	/// Create a new OpenBlock ready for transaction pushing.
	pub fn new<'a, 'b>(engine: &'a Engine, db: OverlayDB, parent: &Header, last_hashes: &'b LastHashes, author: Address, extra_data: Bytes) -> OpenBlock<'a, 'b> {
		let mut r = OpenBlock {
			block: Block::new(State::from_existing(db, parent.state_root().clone(), engine.account_start_nonce())),
			engine: engine,
			last_hashes: last_hashes,
		};

		r.block.header.set_number(parent.number() + 1);
		r.block.header.set_author(author);
		r.block.header.set_extra_data(extra_data);
		r.block.header.set_timestamp_now();

		engine.populate_from_parent(&mut r.block.header, parent);
		engine.on_new_block(&mut r.block);
		r
	}

	/// Alter the author for the block.
	pub fn set_author(&mut self, author: Address) { self.block.header.set_author(author); }

	/// Alter the timestamp of the block.
	pub fn set_timestamp(&mut self, timestamp: u64) { self.block.header.set_timestamp(timestamp); }

	/// Alter the difficulty for the block.
	pub fn set_difficulty(&mut self, a: U256) { self.block.header.set_difficulty(a); }

	/// Alter the gas limit for the block.
	pub fn set_gas_limit(&mut self, a: U256) { self.block.header.set_gas_limit(a); }

	/// Alter the gas limit for the block.
	pub fn set_gas_used(&mut self, a: U256) { self.block.header.set_gas_used(a); }

	/// Alter the extra_data for the block.
	pub fn set_extra_data(&mut self, extra_data: Bytes) -> Result<(), BlockError> {
		if extra_data.len() > self.engine.maximum_extra_data_size() {
			Err(BlockError::ExtraDataOutOfBounds(OutOfBounds{min: None, max: Some(self.engine.maximum_extra_data_size()), found: extra_data.len()}))
		} else {
			self.block.header.set_extra_data(extra_data);
			Ok(())
		}
	}

	/// Add an uncle to the block, if possible.
	///
	/// NOTE Will check chain constraints and the uncle number but will NOT check
	/// that the header itself is actually valid.
	pub fn push_uncle(&mut self, valid_uncle_header: Header) -> Result<(), BlockError> {
		if self.block.uncles.len() >= self.engine.maximum_uncle_count() {
			return Err(BlockError::TooManyUncles(OutOfBounds{min: None, max: Some(self.engine.maximum_uncle_count()), found: self.block.uncles.len()}));
		}
		// TODO: check number
		// TODO: check not a direct ancestor (use last_hashes for that)
		self.block.uncles.push(valid_uncle_header);
		Ok(())
	}

	/// Get the environment info concerning this block.
	pub fn env_info(&self) -> EnvInfo {
		// TODO: memoise.
		EnvInfo {
			number: self.block.header.number,
			author: self.block.header.author.clone(),
			timestamp: self.block.header.timestamp,
			difficulty: self.block.header.difficulty.clone(),
			last_hashes: self.last_hashes.clone(),		// TODO: should be a reference.
			gas_used: self.block.archive.last().map_or(U256::zero(), |t| t.receipt.gas_used),
			gas_limit: self.block.header.gas_limit.clone(),
		}
	}

	/// Push a transaction into the block.
	///
	/// If valid, it will be executed, and archived together with the receipt.
	pub fn push_transaction(&mut self, t: Transaction, h: Option<H256>) -> Result<&Receipt, Error> {
		let env_info = self.env_info();
//		info!("env_info says gas_used={}", env_info.gas_used);
		match self.block.state.apply(&env_info, self.engine, &t) {
			Ok(receipt) => {
				self.block.archive_set.insert(h.unwrap_or_else(||t.hash()));
				self.block.archive.push(Entry { transaction: t, receipt: receipt });
				Ok(&self.block.archive.last().unwrap().receipt)
			}
			Err(x) => Err(From::from(x))
		}
	}

	/// Turn this into a `ClosedBlock`. A BlockChain must be provided in order to figure out the uncles.
	pub fn close(self) -> ClosedBlock<'x, 'y> {
		let mut s = self;
		s.engine.on_close_block(&mut s.block);
		s.block.header.transactions_root = ordered_trie_root(s.block.archive.iter().map(|ref e| e.transaction.rlp_bytes()).collect());
		let uncle_bytes = s.block.uncles.iter().fold(RlpStream::new_list(s.block.uncles.len()), |mut s, u| {s.append(&u.rlp(Seal::With)); s} ).out();
		s.block.header.uncles_hash = uncle_bytes.sha3();
		s.block.header.state_root = s.block.state.root().clone();
		s.block.header.receipts_root = ordered_trie_root(s.block.archive.iter().map(|ref e| e.receipt.rlp_bytes()).collect());
		s.block.header.log_bloom = s.block.archive.iter().fold(LogBloom::zero(), |mut b, e| {b |= &e.receipt.log_bloom; b});
		s.block.header.gas_used = s.block.archive.last().map_or(U256::zero(), |t| t.receipt.gas_used);
		s.block.header.note_dirty();

		ClosedBlock::new(s, uncle_bytes)
	}
}

impl<'x, 'y> IsBlock for OpenBlock<'x, 'y> {
	fn block(&self) -> &Block { &self.block }
}

impl<'x, 'y> IsBlock for ClosedBlock<'x, 'y> {
	fn block(&self) -> &Block { &self.open_block.block }
}

impl<'x, 'y> ClosedBlock<'x, 'y> {
	fn new<'a, 'b>(open_block: OpenBlock<'a, 'b>, uncle_bytes: Bytes) -> ClosedBlock<'a, 'b> {
		ClosedBlock {
			open_block: open_block,
			uncle_bytes: uncle_bytes,
		}
	}

	/// Get the hash of the header without seal arguments.
	pub fn hash(&self) -> H256 { self.header().rlp_sha3(Seal::Without) }

	/// Provide a valid seal in order to turn this into a `SealedBlock`.
	///
	/// NOTE: This does not check the validity of `seal` with the engine.
	pub fn seal(self, seal: Vec<Bytes>) -> Result<SealedBlock, BlockError> {
		let mut s = self;
		if seal.len() != s.open_block.engine.seal_fields() {
			return Err(BlockError::InvalidSealArity(Mismatch{expected: s.open_block.engine.seal_fields(), found: seal.len()}));
		}
		s.open_block.block.header.set_seal(seal);
		Ok(SealedBlock { block: s.open_block.block, uncle_bytes: s.uncle_bytes })
	}

	/// Turn this back into an `OpenBlock`.
	pub fn reopen(self) -> OpenBlock<'x, 'y> { self.open_block }

	/// Drop this object and return the underlieing database.
	pub fn drain(self) -> OverlayDB { self.open_block.block.state.drop().1 }
}

impl SealedBlock {
	/// Get the RLP-encoding of the block.
	pub fn rlp_bytes(&self) -> Bytes {
		let mut block_rlp = RlpStream::new_list(3);
		self.block.header.stream_rlp(&mut block_rlp, Seal::With);
		block_rlp.append_list(self.block.archive.len());
		for e in &self.block.archive { e.transaction.rlp_append(&mut block_rlp); }
		block_rlp.append_raw(&self.uncle_bytes, 1);
		block_rlp.out()
	}

	/// Drop this object and return the underlieing database.
	pub fn drain(self) -> OverlayDB { self.block.state.drop().1 }
}

impl IsBlock for SealedBlock {
	fn block(&self) -> &Block { &self.block }
}

/// Enact the block given by block header, transactions and uncles
pub fn enact<'x, 'y>(header: &Header, transactions: &[Transaction], uncles: &[Header], engine: &'x Engine, db: OverlayDB, parent: &Header, last_hashes: &'y LastHashes) -> Result<ClosedBlock<'x, 'y>, Error> {
	{
		let s = State::from_existing(db.clone(), parent.state_root().clone(), engine.account_start_nonce());
		trace!("enact(): root={}, author={}, author_balance={}\n", s.root(), header.author(), s.balance(&header.author()));
	}

	let mut b = OpenBlock::new(engine, db, parent, last_hashes, header.author().clone(), header.extra_data().clone());
	b.set_difficulty(*header.difficulty());
	b.set_gas_limit(*header.gas_limit());
	b.set_timestamp(header.timestamp());
	for t in transactions { try!(b.push_transaction(t.clone(), None)); }
	for u in uncles { try!(b.push_uncle(u.clone())); }
	Ok(b.close())
}

/// Enact the block given by `block_bytes` using `engine` on the database `db` with given `parent` block header
pub fn enact_bytes<'x, 'y>(block_bytes: &[u8], engine: &'x Engine, db: OverlayDB, parent: &Header, last_hashes: &'y LastHashes) -> Result<ClosedBlock<'x, 'y>, Error> {
	let block = BlockView::new(block_bytes);
	let header = block.header();
	enact(&header, &block.transactions(), &block.uncles(), engine, db, parent, last_hashes)
}

/// Enact the block given by `block_bytes` using `engine` on the database `db` with given `parent` block header
pub fn enact_verified<'x, 'y>(block: &PreVerifiedBlock, engine: &'x Engine, db: OverlayDB, parent: &Header, last_hashes: &'y LastHashes) -> Result<ClosedBlock<'x, 'y>, Error> {
	let view = BlockView::new(&block.bytes);
	enact(&block.header, &block.transactions, &view.uncles(), engine, db, parent, last_hashes)
}

/// Enact the block given by `block_bytes` using `engine` on the database `db` with given `parent` block header. Seal the block aferwards
pub fn enact_and_seal(block_bytes: &[u8], engine: &Engine, db: OverlayDB, parent: &Header, last_hashes: &LastHashes) -> Result<SealedBlock, Error> {
	let header = BlockView::new(block_bytes).header_view();
	Ok(try!(try!(enact_bytes(block_bytes, engine, db, parent, last_hashes)).seal(header.seal())))
}

#[test]
fn open_block() {
	use spec::*;
	let engine = Spec::new_test().to_engine().unwrap();
	let genesis_header = engine.spec().genesis_header();
	let mut db = OverlayDB::new_temp();
	engine.spec().ensure_db_good(&mut db);
	let last_hashes = vec![genesis_header.hash()];
	let b = OpenBlock::new(engine.deref(), db, &genesis_header, &last_hashes, Address::zero(), vec![]);
	let b = b.close();
	let _ = b.seal(vec![]);
}

#[test]
fn enact_block() {
	use spec::*;
	let engine = Spec::new_test().to_engine().unwrap();
	let genesis_header = engine.spec().genesis_header();

	let mut db = OverlayDB::new_temp();
	engine.spec().ensure_db_good(&mut db);
	let b = OpenBlock::new(engine.deref(), db, &genesis_header, &vec![genesis_header.hash()], Address::zero(), vec![]).close().seal(vec![]).unwrap();
	let orig_bytes = b.rlp_bytes();
	let orig_db = b.drain();

	let mut db = OverlayDB::new_temp();
	engine.spec().ensure_db_good(&mut db);
	let e = enact_and_seal(&orig_bytes, engine.deref(), db, &genesis_header, &vec![genesis_header.hash()]).unwrap();

	assert_eq!(e.rlp_bytes(), orig_bytes);
	
	let db = e.drain();
	assert_eq!(orig_db.keys(), db.keys());
	assert!(orig_db.keys().iter().filter(|k| orig_db.get(k.0) != db.get(k.0)).next() == None);
}
