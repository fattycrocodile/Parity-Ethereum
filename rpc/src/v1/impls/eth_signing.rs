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

//! Eth Signing RPC implementation.

use std::sync::{Arc, Weak};
use jsonrpc_core::*;
use ethcore::miner::MinerService;
use ethcore::client::MiningBlockChainClient;
use util::{U256, Address, H256, Mutex};
use transient_hashmap::TransientHashMap;
use ethcore::account_provider::AccountProvider;
use v1::helpers::{SigningQueue, ConfirmationPromise, ConfirmationResult, ConfirmationsQueue, TransactionRequest as TRequest};
use v1::traits::EthSigning;
use v1::types::{TransactionRequest, H160 as RpcH160, H256 as RpcH256, H520 as RpcH520, U256 as RpcU256};
use v1::impls::{default_gas_price, sign_and_dispatch};

fn fill_optional_fields<C, M>(request: &mut TRequest, client: &C, miner: &M)
	where C: MiningBlockChainClient, M: MinerService {
	if request.value.is_none() {
		request.value = Some(U256::from(0));
	}
	if request.gas.is_none() {
		request.gas = Some(miner.sensible_gas_limit());
	}
	if request.gas_price.is_none() {
		request.gas_price = Some(default_gas_price(client, miner));
	}
	if request.data.is_none() {
		request.data = Some(Vec::new());
	}
}

/// Implementation of functions that require signing when no trusted signer is used.
pub struct EthSigningQueueClient<C, M> where C: MiningBlockChainClient, M: MinerService {
	queue: Weak<ConfirmationsQueue>,
	accounts: Weak<AccountProvider>,
	client: Weak<C>,
	miner: Weak<M>,

	pending: Mutex<TransientHashMap<U256, ConfirmationPromise>>,
}

const MAX_PENDING_DURATION: u64 = 60 * 60;

impl<C, M> EthSigningQueueClient<C, M> where C: MiningBlockChainClient, M: MinerService {
	/// Creates a new signing queue client given shared signing queue.
	pub fn new(queue: &Arc<ConfirmationsQueue>, client: &Arc<C>, miner: &Arc<M>, accounts: &Arc<AccountProvider>) -> Self {
		EthSigningQueueClient {
			queue: Arc::downgrade(queue),
			accounts: Arc::downgrade(accounts),
			client: Arc::downgrade(client),
			miner: Arc::downgrade(miner),
			pending: Mutex::new(TransientHashMap::new(MAX_PENDING_DURATION)),
		}
	}

	fn active(&self) -> Result<(), Error> {
		// TODO: only call every 30s at most.
		take_weak!(self.client).keep_alive();
		Ok(())
	}

	fn dispatch<F: FnOnce(ConfirmationPromise) -> Result<Value, Error>>(&self, params: Params, f: F) -> Result<Value, Error> {
		from_params::<(TransactionRequest, )>(params)
			.and_then(|(request, )| {
				let mut request: TRequest = request.into();
				let accounts = take_weak!(self.accounts);
				let (client, miner) = (take_weak!(self.client), take_weak!(self.miner));

				if accounts.is_unlocked(request.from) {
					let sender = request.from;
					return sign_and_dispatch(&*client, &*miner, request, &*accounts, sender);
				}

				let queue = take_weak!(self.queue);
				fill_optional_fields(&mut request, &*client, &*miner);
				let promise = queue.add_request(request);
				f(promise)
			})
	}
}

impl<C, M> EthSigning for EthSigningQueueClient<C, M>
	where C: MiningBlockChainClient + 'static, M: MinerService + 'static
{

	fn sign(&self, _params: Params) -> Result<Value, Error> {
		try!(self.active());
		warn!("Invoking eth_sign is not yet supported with signer enabled.");
		// TODO [ToDr] Implement sign when rest of the signing queue is ready.
		rpc_unimplemented!()
	}

	fn send_transaction(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());
		self.dispatch(params, |promise| {
			promise.wait_with_timeout().unwrap_or_else(|| to_value(&RpcH256::default()))
		})
	}

	fn post_transaction(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());
		self.dispatch(params, |promise| {
			let id = promise.id();
			self.pending.lock().insert(id, promise);
			to_value(&RpcU256::from(id))
		})
	}

	fn check_transaction(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());
		let mut pending = self.pending.lock();
		from_params::<(RpcU256, )>(params).and_then(|(id, )| {
			let id: U256 = id.into();
			let res = match pending.get(&id) {
				Some(ref promise) => match promise.result() {
					ConfirmationResult::Waiting => { return Ok(Value::Null); }
					ConfirmationResult::Rejected => to_value(&RpcH256::default()),
					ConfirmationResult::Confirmed(rpc_response) => rpc_response,
				},
				_ => { return Err(Error::invalid_params()); }
			};
			pending.remove(&id);
			res
		})
	}
}

/// Implementation of functions that require signing when no trusted signer is used.
pub struct EthSigningUnsafeClient<C, M> where
	C: MiningBlockChainClient,
	M: MinerService {
	client: Weak<C>,
	accounts: Weak<AccountProvider>,
	miner: Weak<M>,
}

impl<C, M> EthSigningUnsafeClient<C, M> where
	C: MiningBlockChainClient,
	M: MinerService {

	/// Creates new EthClient.
	pub fn new(client: &Arc<C>, accounts: &Arc<AccountProvider>, miner: &Arc<M>)
		-> Self {
		EthSigningUnsafeClient {
			client: Arc::downgrade(client),
			miner: Arc::downgrade(miner),
			accounts: Arc::downgrade(accounts),
		}
	}

	fn active(&self) -> Result<(), Error> {
		// TODO: only call every 30s at most.
		take_weak!(self.client).keep_alive();
		Ok(())
	}
}

impl<C, M> EthSigning for EthSigningUnsafeClient<C, M> where
	C: MiningBlockChainClient + 'static,
	M: MinerService + 'static {

	fn sign(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());
		from_params::<(RpcH160, RpcH256)>(params).and_then(|(address, msg)| {
			let address: Address = address.into();
			let msg: H256 = msg.into();
			to_value(&take_weak!(self.accounts).sign(address, msg).ok().map_or_else(RpcH520::default, Into::into))
		})
	}

	fn send_transaction(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());
		from_params::<(TransactionRequest, )>(params)
			.and_then(|(request, )| {
				let request: TRequest = request.into();
				let sender = request.from;
				sign_and_dispatch(&*take_weak!(self.client), &*take_weak!(self.miner), request, &*take_weak!(self.accounts), sender)
			})
	}

	fn post_transaction(&self, _: Params) -> Result<Value, Error> {
		// We don't support this in non-signer mode.
		Err(Error::invalid_params())
	}

	fn check_transaction(&self, _: Params) -> Result<Value, Error> {
		// We don't support this in non-signer mode.
		Err(Error::invalid_params())
	}
}
