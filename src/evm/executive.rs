//! Transaction Execution environment.
use common::*;
use state::*;
use engine::*;
use evm::{Schedule, VmFactory, Ext, EvmResult, EvmError};

/// Returns new address created from address and given nonce.
pub fn contract_address(address: &Address, nonce: &U256) -> Address {
	let mut stream = RlpStream::new_list(2);
	stream.append(address);
	stream.append(nonce);
	From::from(stream.out().sha3())
}

/// State changes which should be applied in finalize,
/// after transaction is fully executed.
pub struct Substate {
	/// Any accounts that have suicided.
	suicides: HashSet<Address>,
	/// Any logs.
	logs: Vec<LogEntry>,
	/// Refund counter of SSTORE nonzero->zero.
	refunds_count: U256,
}

impl Substate {
	/// Creates new substate.
	pub fn new() -> Self {
		Substate {
			suicides: HashSet::new(),
			logs: vec![],
			refunds_count: U256::zero(),
		}
	}

	// TODO: remove
	pub fn logs(&self) -> &[LogEntry] {
		&self.logs
	}
}

/// Transaction execution result.
pub struct Executed {
	/// Gas paid up front for execution of transaction.
	pub gas: U256,
	/// Gas used during execution of transaction.
	pub gas_used: U256,
	/// Gas refunded after the execution of transaction. 
	/// To get gas that was required up front, add `refunded` and `gas_used`.
	pub refunded: U256,
	/// Cumulative gas used in current block so far.
	/// 
	/// cumulative_gas_used = gas_used(t0) + gas_used(t1) + ... gas_used(tn)
	///
	/// where `tn` is current transaction.
	pub cumulative_gas_used: U256,
	/// Vector of logs generated by transaction.
	pub logs: Vec<LogEntry>
}

/// Result of executing the transaction.
#[derive(PartialEq, Debug)]
pub enum ExecutionError {
	/// Returned when block (gas_used + gas) > gas_limit.
	/// 
	/// If gas =< gas_limit, upstream may try to execute the transaction
	/// in next block.
	BlockGasLimitReached { gas_limit: U256, gas_used: U256, gas: U256 },
	/// Returned when transaction nonce does not match state nonce.
	InvalidNonce { expected: U256, is: U256 },
	/// Returned when cost of transaction (value + gas_price * gas) exceeds 
	/// current sender balance.
	NotEnoughCash { required: U256, is: U256 },
	/// Returned when transaction execution runs out of gas.
	OutOfGas,
	/// Returned when internal evm error occurs.
	Internal
}

pub type ExecutionResult = Result<Executed, ExecutionError>;

/// Message-call/contract-creation executor; useful for executing transactions.
pub struct Executive<'a> {
	state: &'a mut State,
	info: &'a EnvInfo,
	engine: &'a Engine,
	depth: usize,
}

impl<'a> Executive<'a> {
	/// Creates new executive with depth equal 0.
	pub fn new(state: &'a mut State, info: &'a EnvInfo, engine: &'a Engine) -> Self {
		Executive::new_with_depth(state, info, engine, 0)
	}

	/// Populates executive from parent properties. Increments executive depth.
	fn from_parent(state: &'a mut State, info: &'a EnvInfo, engine: &'a Engine, depth: usize) -> Self {
		Executive::new_with_depth(state, info, engine, depth + 1)
	}

	/// Helper constructor. Should be used to create `Executive` with desired depth.
	/// Private.
	fn new_with_depth(state: &'a mut State, info: &'a EnvInfo, engine: &'a Engine, depth: usize) -> Self {
		Executive {
			state: state,
			info: info,
			engine: engine,
			depth: depth,
		}
	}

	/// This funtion should be used to execute transaction.
	pub fn transact(&mut self, t: &Transaction) -> ExecutionResult {
		// TODO: validate transaction signature ?/ sender

		let sender = t.sender();
		let nonce = self.state.nonce(&sender);

		// validate transaction nonce
		if t.nonce != nonce {
			return Err(ExecutionError::InvalidNonce { expected: nonce, is: t.nonce });
		}
		
		// validate if transaction fits into given block
		if self.info.gas_used + t.gas > self.info.gas_limit {
			return Err(ExecutionError::BlockGasLimitReached { 
				gas_limit: self.info.gas_limit, 
				gas_used: self.info.gas_used, 
				gas: t.gas 
			});
		}

		// TODO: we might need bigints here, or at least check overflows.
		let balance = self.state.balance(&sender);
		let gas_cost = t.gas * t.gas_price;
		let total_cost = t.value + gas_cost;

		// avoid unaffordable transactions
		if balance < total_cost {
			return Err(ExecutionError::NotEnoughCash { required: total_cost, is: balance });
		}

		// NOTE: there can be no invalid transactions from this point.
		self.state.inc_nonce(&sender);
		let mut substate = Substate::new();

		let backup = self.state.clone();

		let res = match t.action() {
			&Action::Create => {
				let params = ActionParams {
					address: contract_address(&sender, &nonce),
					sender: sender.clone(),
					origin: sender.clone(),
					gas: t.gas,
					gas_price: t.gas_price,
					value: t.value,
					code: t.data.clone(),
					data: vec![],
				};
				self.create(&params, &mut substate)
			},
			&Action::Call(ref address) => {
				let params = ActionParams {
					address: address.clone(),
					sender: sender.clone(),
					origin: sender.clone(),
					gas: t.gas,
					gas_price: t.gas_price,
					value: t.value,
					code: self.state.code(address).unwrap_or(vec![]),
					data: t.data.clone(),
				};
				self.call(&params, &mut substate, &mut [])
			}
		};

		// finalize here!
		self.finalize(t, substate, backup, res)
	}

	/// Calls contract function with given contract params.
	/// NOTE. It does not finalize the transaction (doesn't do refunds, nor suicides).
	/// Modifies the substate and the output.
	/// Returns either gas_left or `EvmError`.
	fn call(&mut self, params: &ActionParams, substate: &mut Substate, output: &mut [u8]) -> EvmResult {
		// at first, transfer value to destination
		self.state.transfer_balance(&params.sender, &params.address, &params.value);

		if self.engine.is_builtin(&params.address) {
			// if destination is builtin, try to execute it
			let cost = self.engine.cost_of_builtin(&params.address, &params.data);
			match cost <= params.gas {
				true => {
					self.engine.execute_builtin(&params.address, &params.data, output);
					Ok(params.gas - cost)
				},
				false => Err(EvmError::OutOfGas)
			}
		} else if params.code.len() > 0 {
			// if destination is a contract, do normal message call
			let mut ext = Externalities::from_executive(self, params, substate, OutputPolicy::Return(output));
			let evm = VmFactory::create();
			evm.exec(&params, &mut ext)
		} else {
			// otherwise, nothing
			Ok(params.gas)
		}
	}
	
	/// Creates contract with given contract params.
	/// NOTE. It does not finalize the transaction (doesn't do refunds, nor suicides).
	/// Modifies the substate.
	fn create(&mut self, params: &ActionParams, substate: &mut Substate) -> EvmResult {
		// at first create new contract
		self.state.new_contract(&params.address);
		// then transfer value to it
		self.state.transfer_balance(&params.sender, &params.address, &params.value);

		let mut ext = Externalities::from_executive(self, params, substate, OutputPolicy::InitContract);
		let evm = VmFactory::create();
		evm.exec(&params, &mut ext)
	}

	/// Finalizes the transaction (does refunds and suicides).
	fn finalize(&mut self, t: &Transaction, substate: Substate, backup: State, result: EvmResult) -> ExecutionResult {
		match result { 
			Err(EvmError::Internal) => Err(ExecutionError::Internal),
			Err(EvmError::OutOfGas) => {
				*self.state = backup;
				Err(ExecutionError::OutOfGas)
			},
			Ok(gas_left) => {
				let schedule = self.engine.schedule(self.info);

				// refunds from SSTORE nonzero -> zero
				let sstore_refunds = U256::from(schedule.sstore_refund_gas) * substate.refunds_count;
				// refunds from contract suicides
				let suicide_refunds = U256::from(schedule.suicide_refund_gas) * U256::from(substate.suicides.len());

				// real ammount to refund
				let refund = cmp::min(sstore_refunds + suicide_refunds, (t.gas - gas_left) / U256::from(2)) + gas_left;
				let refund_value = refund * t.gas_price;
				self.state.add_balance(&t.sender(), &refund_value);
				
				// fees earned by author
				let fees = (t.gas - refund) * t.gas_price;
				let author = &self.info.author;
				self.state.add_balance(author, &fees);

				// perform suicides
				for address in substate.suicides.iter() {
					self.state.kill_account(address);
				}

				let gas_used = t.gas - gas_left;
				Ok(Executed {
					gas: t.gas,
					gas_used: gas_used,
					refunded: refund,
					cumulative_gas_used: self.info.gas_used + gas_used,
					logs: substate.logs
				})
			}
		}
	}
}

/// Policy for handling output data on `RETURN` opcode.
pub enum OutputPolicy<'a> {
	/// Return reference to fixed sized output.
	/// Used for message calls.
	Return(&'a mut [u8]),
	/// Init new contract as soon as `RETURN` is called.
	InitContract
}

/// Implementation of evm Externalities.
pub struct Externalities<'a> {
	state: &'a mut State,
	info: &'a EnvInfo,
	engine: &'a Engine,
	depth: usize,
	params: &'a ActionParams,
	substate: &'a mut Substate,
	schedule: Schedule,
	output: OutputPolicy<'a>
}

impl<'a> Externalities<'a> {
	/// Basic `Externalities` constructor.
	pub fn new(state: &'a mut State, 
			   info: &'a EnvInfo, 
			   engine: &'a Engine, 
			   depth: usize, 
			   params: &'a ActionParams, 
			   substate: &'a mut Substate, 
			   output: OutputPolicy<'a>) -> Self {
		Externalities {
			state: state,
			info: info,
			engine: engine,
			depth: depth,
			params: params,
			substate: substate,
			schedule: engine.schedule(info),
			output: output
		}
	}

	/// Creates `Externalities` from `Executive`.
	pub fn from_executive(e: &'a mut Executive, params: &'a ActionParams, substate: &'a mut Substate, output: OutputPolicy<'a>) -> Self {
		Self::new(e.state, e.info, e.engine, e.depth, params, substate, output)
	}
}

impl<'a> Ext for Externalities<'a> {
	fn sload(&self, key: &H256) -> H256 {
		self.state.storage_at(&self.params.address, key)
	}

	fn sstore(&mut self, key: H256, value: H256) {
		// if SSTORE nonzero -> zero, increment refund count
		if value == H256::new() && self.state.storage_at(&self.params.address, &key) != H256::new() {
			self.substate.refunds_count = self.substate.refunds_count + U256::one();
		}
		self.state.set_storage(&self.params.address, key, value)
	}

	fn balance(&self, address: &Address) -> U256 {
		self.state.balance(address)
	}

	fn blockhash(&self, number: &U256) -> H256 {
		match *number < U256::from(self.info.number) {
			false => H256::from(&U256::zero()),
			true => {
				let index = U256::from(self.info.number) - *number - U256::one();
				self.info.last_hashes[index.low_u32() as usize].clone()
			}
		}
	}

	fn create(&mut self, gas: u64, value: &U256, code: &[u8]) -> Result<(u64, Option<Address>), EvmError> {
		// if balance is insufficient or we are to deep, return
		if self.state.balance(&self.params.address) < *value && self.depth >= 1024 {
			return Ok((gas, None));
		}

		// create new contract address
		let address = contract_address(&self.params.address, &self.state.nonce(&self.params.address));

		// prepare the params
		let params = ActionParams {
			address: address.clone(),
			sender: self.params.address.clone(),
			origin: self.params.origin.clone(),
			gas: U256::from(gas),
			gas_price: self.params.gas_price.clone(),
			value: value.clone(),
			code: code.to_vec(),
			data: vec![],
		};

		let mut ex = Executive::from_parent(self.state, self.info, self.engine, self.depth);
		ex.state.inc_nonce(&self.params.address);
		ex.create(&params, self.substate).map(|gas_left| (gas_left.low_u64(), Some(address)))
	}

	fn call(&mut self, gas: u64, call_gas: u64, receive_address: &Address, value: &U256, data: &[u8], code_address: &Address, output: &mut [u8]) -> Result<u64, EvmError> {
		let mut gas_cost = call_gas;
		let mut call_gas = call_gas;

		let is_call = receive_address == code_address;
		if is_call && self.state.code(&code_address).is_none() {
			gas_cost = gas_cost + self.schedule.call_new_account_gas as u64;
		}

		if *value > U256::zero() {
			assert!(self.schedule.call_value_transfer_gas > self.schedule.call_stipend, "overflow possible");
			gas_cost = gas_cost + self.schedule.call_value_transfer_gas as u64;
			call_gas = call_gas + self.schedule.call_stipend as u64;
		}

		if gas_cost > gas {
			return Err(EvmError::OutOfGas)
		}

		let gas = gas - gas_cost;

		//println!("depth: {:?}", self.depth);
		// if balance is insufficient or we are to deep, return
		if self.state.balance(&self.params.address) < *value && self.depth >= 1024 {
			return Ok(gas + call_gas)
		}

		let params = ActionParams {
			address: receive_address.clone(), 
			sender: self.params.address.clone(),
			origin: self.params.origin.clone(),
			gas: U256::from(call_gas),
			gas_price: self.params.gas_price.clone(),
			value: value.clone(),
			code: self.state.code(code_address).unwrap_or(vec![]),
			data: data.to_vec(),
		};

		let mut ex = Executive::from_parent(self.state, self.info, self.engine, self.depth);
		ex.call(&params, self.substate, output).map(|gas_left| gas + gas_left.low_u64())
	}

	fn extcode(&self, address: &Address) -> Vec<u8> {
		self.state.code(address).unwrap_or(vec![])
	}

	fn ret(&mut self, gas: u64, data: &[u8]) -> Result<u64, EvmError> {
		match &mut self.output {
			&mut OutputPolicy::Return(ref mut slice) => unsafe {
				let len = cmp::min(slice.len(), data.len());
				ptr::copy(data.as_ptr(), slice.as_mut_ptr(), len);
				Ok(gas)
			},
			&mut OutputPolicy::InitContract => {
				let return_cost = data.len() as u64 * self.schedule.create_data_gas as u64;
				if return_cost > gas {
					return Err(EvmError::OutOfGas);
				}
				let mut code = vec![];
				code.reserve(data.len());
				unsafe {
					ptr::copy(data.as_ptr(), code.as_mut_ptr(), data.len());
					code.set_len(data.len());
				}
				let address = &self.params.address;
				self.state.init_code(address, code);
				Ok(gas - return_cost)
			}
		}
	}

	fn log(&mut self, topics: Vec<H256>, data: Bytes) {
		let address = self.params.address.clone();
		self.substate.logs.push(LogEntry::new(address, topics, data));
	}

	fn suicide(&mut self) {
		let address = self.params.address.clone();
		self.substate.suicides.insert(address);
	}

	fn schedule(&self) -> &Schedule {
		&self.schedule
	}
}

#[cfg(test)]
mod tests {
	use rustc_serialize::hex::FromHex;
	use std::str::FromStr;
	use util::hash::*;
	use util::uint::*;
	use evm::*;
	use env_info::*;
	use state::*;
	use super::contract_address;
	use ethereum;
	use null_engine::*;
	use std::ops::*;

	#[test]
	fn test_contract_address() {
		let address = Address::from_str("0f572e5295c57f15886f9b263e2f6d2d6c7b5ec6").unwrap();
		let expected_address = Address::from_str("3f09c73a5ed19289fb9bdc72f1742566df146f56").unwrap();
		assert_eq!(expected_address, contract_address(&address, &U256::from(88)));
	}

	#[test]
	// TODO: replace params with transactions!
	fn test_executive() {
		let sender = Address::from_str("0f572e5295c57f15886f9b263e2f6d2d6c7b5ec6").unwrap();
		let address = contract_address(&sender, &U256::zero());
		let mut params = ActionParams::new();
		params.address = address.clone();
		params.sender = sender.clone();
		params.gas = U256::from(0x174876e800u64);
		params.code = "3331600055".from_hex().unwrap();
		params.value = U256::from(0x7);
		let mut state = State::new_temp();
		state.add_balance(&sender, &U256::from(0x100u64));
		let info = EnvInfo::new();
		let engine = NullEngine::new_boxed(ethereum::new_frontier());
		let mut substate = Substate::new();

		{
			let mut ex = Executive::new(&mut state, &info, engine.deref());
			let _res = ex.create(&params, &mut substate);
		}

		assert_eq!(state.storage_at(&address, &H256::new()), H256::from(&U256::from(0xf9u64)));
		assert_eq!(state.balance(&sender), U256::from(0xf9));
		assert_eq!(state.balance(&address), U256::from(0x7));
	}

	#[test]
	fn test_create_contract() {
		let sender = Address::from_str("cd1722f3947def4cf144679da39c4c32bdc35681").unwrap();
		let address = contract_address(&sender, &U256::zero());
		let next_address = contract_address(&address, &U256::zero());
		let mut params = ActionParams::new();
		params.address = address.clone();
		params.sender = sender.clone();
		params.origin = sender.clone();
		params.gas = U256::from(0x174876e800u64);
		params.code = "7c601080600c6000396000f3006000355415600957005b60203560003555600052601d60036000f0600055".from_hex().unwrap();
		let mut state = State::new_temp();
		state.add_balance(&sender, &U256::from(0x100u64));
		let info = EnvInfo::new();
		let engine = NullEngine::new_boxed(ethereum::new_frontier());
		let mut substate = Substate::new();

		{
			let mut ex = Executive::new(&mut state, &info, engine.deref());
			let _res = ex.create(&params, &mut substate);
			println!("res: {:?}", _res);
		}
		
		assert_eq!(state.storage_at(&address, &H256::new()), H256::from(next_address.clone()));
		assert_eq!(state.code(&next_address).unwrap(), "6000355415600957005b602035600035".from_hex().unwrap());
		//assert!(false);
	}

	#[test]
	fn test_recursive_bomb1() {
		// 60 01 - push 1
		// 60 00 - push 0
		// 54 - sload 
		// 01 - add
		// 60 00 - push 0
		// 55 - sstore
		// 60 00 - push 0
		// 60 00 - push 0
		// 60 00 - push 0
		// 60 00 - push 0
		// 60 00 - push 0
		// 30 - load address
		// 60 e0 - push e0
		// 5a - get gas
		// 03 - sub
		// f1 - message call (self in this case)
		// 60 01 - push 1
		// 55 - store
		let sender = Address::from_str("cd1722f3947def4cf144679da39c4c32bdc35681").unwrap();
		let code = "600160005401600055600060006000600060003060e05a03f1600155".from_hex().unwrap();
		let address = contract_address(&sender, &U256::zero());
		let mut params = ActionParams::new();
		params.address = address.clone();
		params.sender = sender.clone();
		params.origin = sender.clone();
		params.gas = U256::from(0x590b3);
		params.gas_price = U256::one();
		params.code = code.clone();
		println!("init gas: {:?}", params.gas.low_u64());
		let mut state = State::new_temp();
		state.init_code(&address, code.clone());
		let info = EnvInfo::new();
		let engine = NullEngine::new_boxed(ethereum::new_frontier());
		let mut substate = Substate::new();

		{
			let mut ex = Executive::new(&mut state, &info, engine.deref());
			let _res = ex.call(&params, &mut substate, &mut []);
			println!("res: {:?}", _res);
		}

		assert!(false);

	}
}
