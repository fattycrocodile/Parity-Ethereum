use util::*;
use basic_types::*;
use error::*;
use evm::Schedule;

#[derive(Debug,Clone)]
pub enum Action {
	Create,
	Call(Address),
}

/// A set of information describing an externally-originating message call
/// or contract creation operation.
#[derive(Debug,Clone)]
pub struct Transaction {
	pub nonce: U256,
	pub gas_price: U256,
	pub gas: U256,
	pub action: Action,
	pub value: U256,
	pub data: Bytes,

	// signature
	pub v: u8,
	pub r: U256,
	pub s: U256,

	hash: RefCell<Option<H256>>,
	sender: RefCell<Option<Address>>,
}

impl Transaction {
	pub fn new() -> Self {
		Transaction {
			nonce: x!(0),
			gas_price: x!(0),
			gas: x!(0),
			action: Action::Create,
			value: x!(0),
			data: vec![],
			v: 0,
			r: x!(0),
			s: x!(0),
			hash: RefCell::new(None),
			sender: RefCell::new(None),
		}
	}
	/// Create a new message-call transaction.
	pub fn new_call(to: Address, value: U256, data: Bytes, gas: U256, gas_price: U256, nonce: U256) -> Transaction {
		Transaction {
			nonce: nonce,
			gas_price: gas_price,
			gas: gas,
			action: Action::Call(to),
			value: value,
			data: data,
			v: 0,
			r: x!(0),
			s: x!(0),
			hash: RefCell::new(None),
			sender: RefCell::new(None),
		}
	}

	/// Create a new contract-creation transaction.
	pub fn new_create(value: U256, data: Bytes, gas: U256, gas_price: U256, nonce: U256) -> Transaction {
		Transaction {
			nonce: nonce,
			gas_price: gas_price,
			gas: gas,
			action: Action::Create,
			value: value,
			data: data,
			v: 0,
			r: x!(0),
			s: x!(0),
			hash: RefCell::new(None),
			sender: RefCell::new(None),
		}
	}

	/// Get the nonce of the transaction.
	pub fn nonce(&self) -> &U256 { &self.nonce }
	/// Get the gas price of the transaction.
	pub fn gas_price(&self) -> &U256 { &self.gas_price }
	/// Get the gas of the transaction.
	pub fn gas(&self) -> &U256 { &self.gas }
	/// Get the action of the transaction (Create or Call).
	pub fn action(&self) -> &Action { &self.action }
	/// Get the value of the transaction.
	pub fn value(&self) -> &U256 { &self.value }
	/// Get the data of the transaction.
	pub fn data(&self) -> &Bytes { &self.data }

	/// Append object into RLP stream, optionally with or without the signature.
	pub fn rlp_append_opt(&self, s: &mut RlpStream, with_seal: Seal) {
		s.append_list(6 + match with_seal { Seal::With => 3, _ => 0 });
		s.append(&self.nonce);
		s.append(&self.gas_price);
		s.append(&self.gas);
		match self.action {
			Action::Create => s.append_empty_data(),
			Action::Call(ref to) => s.append(to),
		};
		s.append(&self.value);
		s.append(&self.data);
		match with_seal {
			Seal::With => { s.append(&(self.v as u16)).append(&self.r).append(&self.s); },
			_ => {}
		}
	}

	/// Get the RLP serialisation of the object, optionally with or without the signature.
	pub fn rlp_bytes_opt(&self, with_seal: Seal) -> Bytes {
		let mut s = RlpStream::new();
		self.rlp_append_opt(&mut s, with_seal);
		s.out()
	}
}

impl FromJson for Transaction {
	fn from_json(json: &Json) -> Transaction {
		let mut r = Transaction {
			nonce: xjson!(&json["nonce"]),
			gas_price: xjson!(&json["gasPrice"]),
			gas: xjson!(&json["gasLimit"]),
			action: match Bytes::from_json(&json["to"]) {
				ref x if x.len() == 0 => Action::Create,
				ref x => Action::Call(Address::from_slice(x)),
			},
			value: xjson!(&json["value"]),
			data: xjson!(&json["data"]),
			v: match json.find("v") { Some(ref j) => u16::from_json(j) as u8, None => 0 },
			r: match json.find("r") { Some(j) => xjson!(j), None => x!(0) },
			s: match json.find("s") { Some(j) => xjson!(j), None => x!(0) },
			hash: RefCell::new(None),
			sender: match json.find("sender") {
				Some(&Json::String(ref sender)) => RefCell::new(Some(address_from_hex(clean(sender)))),
				_ => RefCell::new(None),
			},
		};
		if let Some(&Json::String(ref secret_key)) = json.find("secretKey") {
			r.sign(&h256_from_hex(clean(secret_key)));
		}
		r
	}
}

impl RlpStandard for Transaction {
	fn rlp_append(&self, s: &mut RlpStream) { self.rlp_append_opt(s, Seal::With) }
}

impl Transaction {
	/// Get the hash of this header (sha3 of the RLP).
	pub fn hash(&self) -> H256 {
 		let mut hash = self.hash.borrow_mut();
 		match &mut *hash {
 			&mut Some(ref h) => h.clone(),
 			hash @ &mut None => {
 				*hash = Some(self.rlp_sha3());
 				hash.as_ref().unwrap().clone()
 			}
		}
	}

	/// Note that some fields have changed. Resets the memoised hash.
	pub fn note_dirty(&self) {
 		*self.hash.borrow_mut() = None;
	}

	/// 0 is `v` is 27, 1 if 28, and 4 otherwise.
	pub fn standard_v(&self) -> u8 { match self.v { 27 => 0, 28 => 1, _ => 4 } }

	/// Construct a signature object from the sig.
	pub fn signature(&self) -> Signature { Signature::from_rsv(&From::from(&self.r), &From::from(&self.s), self.standard_v()) }

	/// The message hash of the transaction.
	pub fn message_hash(&self) -> H256 { self.rlp_bytes_opt(Seal::Without).sha3() }

	/// Returns transaction sender.
	pub fn sender(&self) -> Result<Address, Error> {
 		let mut sender = self.sender.borrow_mut();
 		match &mut *sender {
 			&mut Some(ref h) => Ok(h.clone()),
 			sender @ &mut None => {
 				*sender = Some(From::from(try!(ec::recover(&self.signature(), &self.message_hash())).sha3()));
 				Ok(sender.as_ref().unwrap().clone())
 			}
		}
	}

	/// Signs the transaction as coming from `sender`.
	pub fn sign(&mut self, secret: &Secret) {
		// TODO: make always low.
		let sig = ec::sign(secret, &self.message_hash());
		let (r, s, v) = sig.unwrap().to_rsv();
		self.r = r;
		self.s = s;
		self.v = v + 27;
	}

	/// Signs the transaction as coming from `sender`.
	pub fn signed(self, secret: &Secret) -> Transaction { let mut r = self; r.sign(secret); r }

	/// Get the transaction cost in gas for the given params.
	pub fn gas_required_for(is_create: bool, data: &[u8], schedule: &Schedule) -> u64 {
		data.iter().fold(
			(if is_create {schedule.tx_create_gas} else {schedule.tx_gas}) as u64,
			|g, b| g + (match *b { 0 => schedule.tx_data_zero_gas, _ => schedule.tx_data_non_zero_gas }) as u64
		)
	}

	/// Get the transaction cost in gas for this transaction.
	pub fn gas_required(&self, schedule: &Schedule) -> u64 {
		Self::gas_required_for(match self.action{Action::Create=>true, Action::Call(_)=>false}, &self.data, schedule)
	}

	/// Do basic validation, checking for valid signature and minimum gas,
	pub fn validate(self, schedule: &Schedule, require_low: bool) -> Result<Transaction, Error> {
		if require_low && !ec::is_low_s(&self.s) {
			return Err(Error::Util(UtilError::Crypto(CryptoError::InvalidSignature)));
		}
		try!(self.sender());
		if self.gas < U256::from(self.gas_required(&schedule)) {
			Err(From::from(TransactionError::InvalidGasLimit(OutOfBounds{min: Some(U256::from(self.gas_required(&schedule))), max: None, found: self.gas})))
		} else {
			Ok(self)
		}
	}
}

impl Decodable for Action {
	fn decode<D>(decoder: &D) -> Result<Self, DecoderError> where D: Decoder {
		let rlp = decoder.as_rlp();
		if rlp.is_empty() {
			Ok(Action::Create)
		} else {
			Ok(Action::Call(try!(rlp.as_val())))
		}
	}
}

impl Decodable for Transaction {
	fn decode<D>(decoder: &D) -> Result<Self, DecoderError> where D: Decoder {
		let d = try!(decoder.as_list());
		if d.len() != 9 {
			return Err(DecoderError::RlpIncorrectListLen);
		}
		Ok(Transaction {
			nonce: try!(Decodable::decode(&d[0])),
			gas_price: try!(Decodable::decode(&d[1])),
			gas: try!(Decodable::decode(&d[2])),
			action: try!(Decodable::decode(&d[3])),
			value: try!(Decodable::decode(&d[4])),
			data: try!(Decodable::decode(&d[5])),
			v: try!(u16::decode(&d[6])) as u8,
			r: try!(Decodable::decode(&d[7])),
			s: try!(Decodable::decode(&d[8])),
			hash: RefCell::new(None),
			sender: RefCell::new(None),
		})
	}
}

#[test]
fn sender_test() {
	let t: Transaction = decode(&FromHex::from_hex("f85f800182520894095e7baea6a6c7c4c2dfeb977efac326af552d870a801ba048b55bfa915ac795c431978d8a6a992b628d557da5ff759b307d495a36649353a0efffd310ac743f371de3b9f7f9cb56c0b28ad43601b4ab949f53faa07bd2c804").unwrap());
	assert_eq!(t.data, b"");
	assert_eq!(t.gas, U256::from(0x5208u64));
	assert_eq!(t.gas_price, U256::from(0x01u64));
	assert_eq!(t.nonce, U256::from(0x00u64));
	if let Action::Call(ref to) = t.action {
		assert_eq!(*to, address_from_hex("095e7baea6a6c7c4c2dfeb977efac326af552d87"));
	} else { panic!(); }
	assert_eq!(t.value, U256::from(0x0au64));
	assert_eq!(t.sender().unwrap(), address_from_hex("0f65fe9276bc9a24ae7083ae28e2660ef72df99e"));
}

#[test]
fn signing() {
	let key = KeyPair::create().unwrap();
	let t = Transaction::new_create(U256::from(42u64), b"Hello!".to_vec(), U256::from(3000u64), U256::from(50_000u64), U256::from(1u64)).signed(&key.secret());
	assert_eq!(Address::from(key.public().sha3()), t.sender().unwrap());
}