use common::*;
use flate2::read::GzDecoder;
use engine::*;
use pod_state::*;
use null_engine::*;

/// Converts file from base64 gzipped bytes to json
pub fn gzip64res_to_json(source: &[u8]) -> Json {
	// there is probably no need to store genesis in based64 gzip,
	// but that's what go does, and it was easy to load it this way
	let data = source.from_base64().expect("Genesis block is malformed!");
	let data_ref: &[u8] = &data;
	let mut decoder = GzDecoder::new(data_ref).expect("Gzip is invalid");
	let mut s: String = "".to_owned();
	decoder.read_to_string(&mut s).expect("Gzip is invalid");
	Json::from_str(&s).expect("Json is invalid")
}

/// Convert JSON value to equivlaent RLP representation.
// TODO: handle container types.
fn json_to_rlp(json: &Json) -> Bytes {
	match *json {
		Json::Boolean(o) => encode(&(if o {1u64} else {0})).to_vec(),
		Json::I64(o) => encode(&(o as u64)).to_vec(),
		Json::U64(o) => encode(&o).to_vec(),
		Json::String(ref s) if s.len() >= 2 && &s[0..2] == "0x" && U256::from_str(&s[2..]).is_ok() => {
			encode(&U256::from_str(&s[2..]).unwrap()).to_vec()
		},
		Json::String(ref s) => {
			encode(s).to_vec()
		},
		_ => panic!()
	}
}

/// Convert JSON to a string->RLP map.
fn json_to_rlp_map(json: &Json) -> HashMap<String, Bytes> {
	json.as_object().unwrap().iter().map(|(k, v)| (k, json_to_rlp(v))).fold(HashMap::new(), |mut acc, kv| {
		acc.insert(kv.0.clone(), kv.1);
		acc
	})
}

/// Parameters for a block chain; includes both those intrinsic to the design of the
/// chain and those to be interpreted by the active chain engine.
#[derive(Debug)]
pub struct Spec {
	// User friendly spec name
	/// TODO [Gav Wood] Please document me
	pub name: String,
	// What engine are we using for this?
	/// TODO [Gav Wood] Please document me
	pub engine_name: String,

	/// Known nodes on the network in enode format.
	pub nodes: Vec<String>,

	// Parameters concerning operation of the specific engine we're using.
	// Name -> RLP-encoded value
	/// TODO [Gav Wood] Please document me
	pub engine_params: HashMap<String, Bytes>,

	// Builtin-contracts are here for now but would like to abstract into Engine API eventually.
	/// TODO [Gav Wood] Please document me
	pub builtins: BTreeMap<Address, Builtin>,

	// Genesis params.
	/// TODO [Gav Wood] Please document me
	pub parent_hash: H256,
	/// TODO [Gav Wood] Please document me
	pub author: Address,
	/// TODO [Gav Wood] Please document me
	pub difficulty: U256,
	/// TODO [Gav Wood] Please document me
	pub gas_limit: U256,
	/// TODO [Gav Wood] Please document me
	pub gas_used: U256,
	/// TODO [Gav Wood] Please document me
	pub timestamp: u64,
	/// TODO [arkpar] Please document me
	pub extra_data: Bytes,
	/// TODO [Gav Wood] Please document me
	genesis_state: PodState,
	/// TODO [Gav Wood] Please document me
	pub seal_fields: usize,
	/// TODO [Gav Wood] Please document me
	pub seal_rlp: Bytes,

	// May be prepopulated if we know this in advance.
	state_root_memo: RwLock<Option<H256>>,
}

#[allow(wrong_self_convention)] // because to_engine(self) should be to_engine(&self)
impl Spec {
	/// Convert this object into a boxed Engine of the right underlying type.
	// TODO avoid this hard-coded nastiness - use dynamic-linked plugin framework instead.
	pub fn to_engine(self) -> Result<Box<Engine>, Error> {
		match self.engine_name.as_ref() {
			"NullEngine" => Ok(NullEngine::new_boxed(self)),
			"Ethash" => Ok(super::ethereum::Ethash::new_boxed(self)),
			_ => Err(Error::UnknownEngineName(self.engine_name.clone()))
		}
	}

	/// Return the state root for the genesis state, memoising accordingly.
	pub fn state_root(&self) -> H256 {
		if self.state_root_memo.read().unwrap().is_none() {
			*self.state_root_memo.write().unwrap() = Some(self.genesis_state.root());
		}
		self.state_root_memo.read().unwrap().as_ref().unwrap().clone()
	}

	/// Get the known knodes of the network in enode format.
	pub fn nodes(&self) -> &Vec<String> { &self.nodes }

	/// TODO [Gav Wood] Please document me
	pub fn genesis_header(&self) -> Header {
		Header {
			parent_hash: self.parent_hash.clone(),
			timestamp: self.timestamp,
			number: 0,
			author: self.author.clone(),
			transactions_root: SHA3_NULL_RLP.clone(),
			uncles_hash: RlpStream::new_list(0).out().sha3(),
			extra_data: self.extra_data.clone(),
			state_root: self.state_root().clone(),
			receipts_root: SHA3_NULL_RLP.clone(),
			log_bloom: H2048::new().clone(),
			gas_used: self.gas_used.clone(),
			gas_limit: self.gas_limit.clone(),
			difficulty: self.difficulty.clone(),
			seal: {
				let seal = {
					let mut s = RlpStream::new_list(self.seal_fields);
					s.append_raw(&self.seal_rlp, self.seal_fields);
					s.out()
				};
				let r = Rlp::new(&seal);
				(0..self.seal_fields).map(|i| r.at(i).as_raw().to_vec()).collect()
			},
			hash: RefCell::new(None),
			bare_hash: RefCell::new(None),
		}
	}

	/// Compose the genesis block for this chain.
	pub fn genesis_block(&self) -> Bytes {
		let empty_list = RlpStream::new_list(0).out();
		let header = self.genesis_header();
		let mut ret = RlpStream::new_list(3);
		ret.append(&header);
		ret.append_raw(&empty_list, 1);
		ret.append_raw(&empty_list, 1);
		ret.out()
	}

	/// Overwrite the genesis components with the given JSON, assuming standard Ethereum test format.
	pub fn overwrite_genesis(&mut self, genesis: &Json) {
		let (seal_fields, seal_rlp) = {
			if genesis.find("mixHash").is_some() && genesis.find("nonce").is_some() {
				let mut s = RlpStream::new();
				s.append(&H256::from_json(&genesis["mixHash"]));
				s.append(&H64::from_json(&genesis["nonce"]));
				(2, s.out())
			} else {
				// backup algo that will work with sealFields/sealRlp (and without).
				(
					u64::from_json(&genesis["sealFields"]) as usize,
					Bytes::from_json(&genesis["sealRlp"])
				)
			}
		};
		
		self.parent_hash = H256::from_json(&genesis["parentHash"]);
		self.author = Address::from_json(&genesis["coinbase"]);
		self.difficulty = U256::from_json(&genesis["difficulty"]);
		self.gas_limit = U256::from_json(&genesis["gasLimit"]);
		self.gas_used = U256::from_json(&genesis["gasUsed"]);
		self.timestamp = u64::from_json(&genesis["timestamp"]);
		self.extra_data = Bytes::from_json(&genesis["extraData"]);
		self.seal_fields = seal_fields;
		self.seal_rlp = seal_rlp;
		self.state_root_memo = RwLock::new(genesis.find("stateRoot").and_then(|_| Some(H256::from_json(&genesis["stateRoot"]))));
	}

	/// Alter the value of the genesis state.
	pub fn set_genesis_state(&mut self, s: PodState) {
		self.genesis_state = s;
		*self.state_root_memo.write().unwrap() = None;
	}

	/// Returns `false` if the memoized state root is invalid. `true` otherwise.
	pub fn is_state_root_valid(&self) -> bool {
		self.state_root_memo.read().unwrap().clone().map_or(true, |sr| sr == self.genesis_state.root())
	}
}

impl FromJson for Spec {
	/// Loads a chain-specification from a json data structure
	fn from_json(json: &Json) -> Spec {
		// once we commit ourselves to some json parsing library (serde?)
		// move it to proper data structure
		let mut builtins = BTreeMap::new();
		let mut state = PodState::new();

		if let Some(&Json::Object(ref accounts)) = json.find("accounts") {
			for (address, acc) in accounts.iter() {
				let addr = Address::from_str(address).unwrap();
				if let Some(ref builtin_json) = acc.find("builtin") {
					if let Some(builtin) = Builtin::from_json(builtin_json) {
						builtins.insert(addr.clone(), builtin);
					}
				}
			}
			state = xjson!(&json["accounts"]);
		}

		let nodes = if let Some(&Json::Array(ref ns)) = json.find("nodes") {
			ns.iter().filter_map(|n| if let Json::String(ref s) = *n { Some(s.clone()) } else {None}).collect()
		} else { Vec::new() };

		let genesis = &json["genesis"];//.as_object().expect("No genesis object in JSON");

		let (seal_fields, seal_rlp) = {
			if genesis.find("mixHash").is_some() && genesis.find("nonce").is_some() {
				let mut s = RlpStream::new();
				s.append(&H256::from_str(&genesis["mixHash"].as_string().expect("mixHash not a string.")[2..]).expect("Invalid mixHash string value"));
				s.append(&H64::from_str(&genesis["nonce"].as_string().expect("nonce not a string.")[2..]).expect("Invalid nonce string value"));
				(2, s.out())
			} else {
				// backup algo that will work with sealFields/sealRlp (and without).
				(
					usize::from_str(&genesis["sealFields"].as_string().unwrap_or("0x")[2..]).expect("Invalid sealFields integer data"),
					genesis["sealRlp"].as_string().unwrap_or("0x")[2..].from_hex().expect("Invalid sealRlp hex data")
				)
			}
		};
		
		Spec {
			name: json.find("name").map_or("unknown", |j| j.as_string().unwrap()).to_owned(),
			engine_name: json["engineName"].as_string().unwrap().to_owned(),
			engine_params: json_to_rlp_map(&json["params"]),
			nodes: nodes,
			builtins: builtins,
			parent_hash: H256::from_str(&genesis["parentHash"].as_string().unwrap()[2..]).unwrap(),
			author: Address::from_str(&genesis["author"].as_string().unwrap()[2..]).unwrap(),
			difficulty: U256::from_str(&genesis["difficulty"].as_string().unwrap()[2..]).unwrap(),
			gas_limit: U256::from_str(&genesis["gasLimit"].as_string().unwrap()[2..]).unwrap(),
			gas_used: U256::from(0u8),
			timestamp: u64::from_str(&genesis["timestamp"].as_string().unwrap()[2..]).unwrap(),
			extra_data: genesis["extraData"].as_string().unwrap()[2..].from_hex().unwrap(),
			genesis_state: state,
			seal_fields: seal_fields,
			seal_rlp: seal_rlp,
			state_root_memo: RwLock::new(genesis.find("stateRoot").and_then(|_| genesis["stateRoot"].as_string()).map(|s| H256::from_str(&s[2..]).unwrap())),
		}
	}
}

impl Spec {
	/// Ensure that the given state DB has the trie nodes in for the genesis state.
	pub fn ensure_db_good(&self, db: &mut HashDB) -> bool {
		if !db.contains(&self.state_root()) {
			let mut root = H256::new(); 
			{
				let mut t = SecTrieDBMut::new(db, &mut root);
				for (address, account) in self.genesis_state.get().iter() {
					t.insert(address.as_slice(), &account.rlp());
				}
			}
			for (_, account) in self.genesis_state.get().iter() {
				account.insert_additional(db);
			}
			assert!(db.contains(&self.state_root()));
			true
		} else { false }
	}

	/// Create a new Spec from a JSON UTF-8 data resource `data`.
	pub fn from_json_utf8(data: &[u8]) -> Spec {
		Self::from_json_str(::std::str::from_utf8(data).unwrap())
	}

	/// Create a new Spec from a JSON string.
	pub fn from_json_str(s: &str) -> Spec {
		Self::from_json(&Json::from_str(s).expect("Json is invalid"))
	}

	/// Create a new Spec which conforms to the Morden chain except that it's a NullEngine consensus.
	pub fn new_test() -> Spec { Self::from_json_utf8(include_bytes!("../res/null_morden.json")) }
}

#[cfg(test)]
mod tests {
	use std::str::FromStr;
	use util::hash::*;
	use util::sha3::*;
	use views::*;
	use super::*;

	#[test]
	fn test_chain() {
		let test_spec = Spec::new_test();

		assert_eq!(test_spec.state_root(), H256::from_str("f3f4696bbf3b3b07775128eb7a3763279a394e382130f27c21e70233e04946a9").unwrap());
		let genesis = test_spec.genesis_block();
		assert_eq!(BlockView::new(&genesis).header_view().sha3(), H256::from_str("0cd786a2425d16f152c658316c423e6ce1181e15c3295826d7c9904cba9ce303").unwrap());

		let _ = test_spec.to_engine();
	}
}
