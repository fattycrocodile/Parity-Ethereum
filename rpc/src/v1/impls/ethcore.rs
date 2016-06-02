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

//! Ethcore-specific rpc implementation.
use util::{U256, Address, RotatingLogger, FixedHash, Uint};
use util::network_settings::NetworkSettings;
use util::misc::version_data;
use std::sync::{Arc, Weak};
use std::ops::Deref;
use std::collections::BTreeMap;
use jsonrpc_core::*;
use ethcore::miner::MinerService;
use ethcore::transaction::{Transaction as EthTransaction, SignedTransaction, Action};
use ethcore::client::BlockChainClient;
use ethcore::trace::VMTrace;
use v1::traits::Ethcore;
use v1::types::{Bytes, CallRequest};

/// Ethcore implementation.
pub struct EthcoreClient<C, M> where
	C: BlockChainClient,
	M: MinerService {

	client: Weak<C>,
	miner: Weak<M>,
	logger: Arc<RotatingLogger>,
	settings: Arc<NetworkSettings>,
}

impl<C, M> EthcoreClient<C, M> where C: BlockChainClient, M: MinerService {
	/// Creates new `EthcoreClient`.
	pub fn new(client: &Arc<C>, miner: &Arc<M>, logger: Arc<RotatingLogger>, settings: Arc<NetworkSettings>) -> Self {
		EthcoreClient {
			client: Arc::downgrade(client),
			miner: Arc::downgrade(miner),
			logger: logger,
			settings: settings,
		}
	}

	// TODO: share with eth.rs
	fn sign_call(&self, request: CallRequest) -> Result<SignedTransaction, Error> {
		let client = take_weak!(self.client);
		let miner = take_weak!(self.miner);
		let from = request.from.unwrap_or(Address::zero());
		Ok(EthTransaction {
			nonce: request.nonce.unwrap_or_else(|| client.latest_nonce(&from)),
			action: request.to.map_or(Action::Create, Action::Call),
			gas: request.gas.unwrap_or(U256::from(50_000_000)),
			gas_price: request.gas_price.unwrap_or_else(|| miner.sensible_gas_price()),
			value: request.value.unwrap_or_else(U256::zero),
			data: request.data.map_or_else(Vec::new, |d| d.to_vec())
		}.fake_sign(from))
	}
}

fn vm_trace_to_object(t: &VMTrace) -> Value {
	let mut ret = BTreeMap::new();
	ret.insert("code".to_owned(), to_value(&t.code).unwrap());

	let mut subs = t.subs.iter();
	let mut next_sub = subs.next();

	let ops = t.operations
		.iter()
		.enumerate()
		.map(|(i, op)| {
			let mut m = map![
				"pc".to_owned() => to_value(&op.pc).unwrap(),
				"cost".to_owned() => match op.gas_cost <= U256::from(!0u64) {
					true => to_value(&op.gas_cost.low_u64()),
					false => to_value(&op.gas_cost),
				}.unwrap()
			];
			if let Some(ref ex) = op.executed {
				let mut em = map![
					"used".to_owned() => to_value(&ex.gas_used.low_u64()).unwrap(),
					"push".to_owned() => to_value(&ex.stack_push).unwrap()
				];
				if let Some(ref md) = ex.mem_diff {
					em.insert("mem".to_owned(), Value::Object(map![
						"off".to_owned() => to_value(&md.offset).unwrap(),
						"data".to_owned() => to_value(&md.data).unwrap()
					]));
				}
				if let Some(ref sd) = ex.store_diff {
					em.insert("store".to_owned(), Value::Object(map![
						"key".to_owned() => to_value(&sd.location).unwrap(),
						"val".to_owned() => to_value(&sd.value).unwrap()
					]));
				}
				m.insert("ex".to_owned(), Value::Object(em));
			}
			if next_sub.is_some() && next_sub.unwrap().parent_step == i {
				m.insert("sub".to_owned(), vm_trace_to_object(next_sub.unwrap()));
				next_sub = subs.next();
			}
			Value::Object(m)
		})
		.collect::<Vec<_>>();
	ret.insert("ops".to_owned(), Value::Array(ops));
	Value::Object(ret)
}

impl<C, M> Ethcore for EthcoreClient<C, M> where C: BlockChainClient + 'static, M: MinerService + 'static {

	fn set_min_gas_price(&self, params: Params) -> Result<Value, Error> {
		from_params::<(U256,)>(params).and_then(|(gas_price,)| {
			take_weak!(self.miner).set_minimal_gas_price(gas_price);
			to_value(&true)
		})
	}

	fn set_gas_floor_target(&self, params: Params) -> Result<Value, Error> {
		from_params::<(U256,)>(params).and_then(|(gas_floor_target,)| {
			take_weak!(self.miner).set_gas_floor_target(gas_floor_target);
			to_value(&true)
		})
	}

	fn set_extra_data(&self, params: Params) -> Result<Value, Error> {
		from_params::<(Bytes,)>(params).and_then(|(extra_data,)| {
			take_weak!(self.miner).set_extra_data(extra_data.to_vec());
			to_value(&true)
		})
	}

	fn set_author(&self, params: Params) -> Result<Value, Error> {
		from_params::<(Address,)>(params).and_then(|(author,)| {
			take_weak!(self.miner).set_author(author);
			to_value(&true)
		})
	}

	fn set_transactions_limit(&self, params: Params) -> Result<Value, Error> {
		from_params::<(usize,)>(params).and_then(|(limit,)| {
			take_weak!(self.miner).set_transactions_limit(limit);
			to_value(&true)
		})
	}

	fn transactions_limit(&self, _: Params) -> Result<Value, Error> {
		to_value(&take_weak!(self.miner).transactions_limit())
	}

	fn min_gas_price(&self, _: Params) -> Result<Value, Error> {
		to_value(&take_weak!(self.miner).minimal_gas_price())
	}

	fn extra_data(&self, _: Params) -> Result<Value, Error> {
		to_value(&Bytes::new(take_weak!(self.miner).extra_data()))
	}

	fn gas_floor_target(&self, _: Params) -> Result<Value, Error> {
		to_value(&take_weak!(self.miner).gas_floor_target())
	}

	fn dev_logs(&self, _params: Params) -> Result<Value, Error> {
		let logs = self.logger.logs();
		to_value(&logs.deref().as_slice())
	}

	fn dev_logs_levels(&self, _params: Params) -> Result<Value, Error> {
		to_value(&self.logger.levels())
	}

	fn net_chain(&self, _params: Params) -> Result<Value, Error> {
		to_value(&self.settings.chain)
	}

	fn net_max_peers(&self, _params: Params) -> Result<Value, Error> {
		to_value(&self.settings.max_peers)
	}

	fn net_port(&self, _params: Params) -> Result<Value, Error> {
		to_value(&self.settings.network_port)
	}

	fn node_name(&self, _params: Params) -> Result<Value, Error> {
		to_value(&self.settings.name)
	}

	fn rpc_settings(&self, _params: Params) -> Result<Value, Error> {
		let mut map = BTreeMap::new();
		map.insert("enabled".to_owned(), Value::Bool(self.settings.rpc_enabled));
		map.insert("interface".to_owned(), Value::String(self.settings.rpc_interface.clone()));
		map.insert("port".to_owned(), Value::U64(self.settings.rpc_port as u64));
		Ok(Value::Object(map))
	}

	fn default_extra_data(&self, _params: Params) -> Result<Value, Error> {
		let version = version_data();
		to_value(&Bytes::new(version))
	}

	fn vm_trace_call(&self, params: Params) -> Result<Value, Error> {
		trace!(target: "jsonrpc", "vm_trace_call: {:?}", params);
		from_params(params)
			.and_then(|(request,)| {
				let signed = try!(self.sign_call(request));
				let r = take_weak!(self.client).call(&signed, true);
				if let Ok(executed) = r {
					if let Some(vm_trace) = executed.vm_trace {
						return Ok(vm_trace_to_object(&vm_trace));
					}
				}
				Ok(Value::Null)
			})
	}
}
