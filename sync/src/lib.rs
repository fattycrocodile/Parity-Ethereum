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

#![warn(missing_docs)]
#![feature(plugin)]
#![plugin(clippy)]
#![feature(augmented_assignments)]
//! Blockchain sync module
//! Implements ethereum protocol version 63 as specified here:
//! https://github.com/ethereum/wiki/wiki/Ethereum-Wire-Protocol
//!
//! Usage example:
//!
//! ```rust
//! extern crate ethcore_util as util;
//! extern crate ethcore;
//! extern crate ethsync;
//! use std::env;
//! use std::sync::Arc;
//! use util::network::{NetworkService, NetworkConfiguration};
//! use ethcore::client::Client;
//! use ethsync::EthSync;
//! use ethcore::ethereum;
//!
//! fn main() {
//! 	let mut service = NetworkService::start(NetworkConfiguration::new()).unwrap();
//! 	let dir = env::temp_dir();
//! 	let client = Client::new(ethereum::new_frontier(), &dir, service.io().channel()).unwrap();
//! 	EthSync::register(&mut service, client);
//! }
//! ```

#[macro_use]
extern crate log;
#[macro_use]
extern crate ethcore_util as util;
extern crate ethcore;
extern crate env_logger;
extern crate time;
extern crate rand;

use std::ops::*;
use std::sync::*;
use ethcore::client::Client;
use util::network::{NetworkProtocolHandler, NetworkService, NetworkContext, PeerId};
use util::io::TimerToken;
use chain::ChainSync;
use ethcore::service::SyncMessage;
use io::NetSyncIo;

mod chain;
mod io;
mod range_collection;

#[cfg(test)]
mod tests;

/// Ethereum network protocol handler
pub struct EthSync {
	/// Shared blockchain client. TODO: this should evetually become an IPC endpoint
	chain: Arc<Client>,
	/// Sync strategy
	sync: RwLock<ChainSync>
}

pub use self::chain::SyncStatus;

impl EthSync {
	/// Creates and register protocol with the network service
	pub fn register(service: &mut NetworkService<SyncMessage>, chain: Arc<Client>) -> Arc<EthSync> {
		let sync = Arc::new(EthSync {
			chain: chain,
			sync: RwLock::new(ChainSync::new()),
		});
		service.register_protocol(sync.clone(), "eth", &[62u8, 63u8]).expect("Error registering eth protocol handler");
		sync
	}

	/// Get sync status
	pub fn status(&self) -> SyncStatus {
		self.sync.read().unwrap().status()
	}

	/// Stop sync
	pub fn stop(&mut self, io: &mut NetworkContext<SyncMessage>) {
		self.sync.write().unwrap().abort(&mut NetSyncIo::new(io, self.chain.deref()));
	}

	/// Restart sync
	pub fn restart(&mut self, io: &mut NetworkContext<SyncMessage>) {
		self.sync.write().unwrap().restart(&mut NetSyncIo::new(io, self.chain.deref()));
	}
}

impl NetworkProtocolHandler<SyncMessage> for EthSync {
	fn initialize(&self, io: &NetworkContext<SyncMessage>) {
		io.register_timer(0, 1000).expect("Error registering sync timer");
	}

	fn read(&self, io: &NetworkContext<SyncMessage>, peer: &PeerId, packet_id: u8, data: &[u8]) {
		self.sync.write().unwrap().on_packet(&mut NetSyncIo::new(io, self.chain.deref()) , *peer, packet_id, data);
	}

	fn connected(&self, io: &NetworkContext<SyncMessage>, peer: &PeerId) {
		self.sync.write().unwrap().on_peer_connected(&mut NetSyncIo::new(io, self.chain.deref()), *peer);
	}

	fn disconnected(&self, io: &NetworkContext<SyncMessage>, peer: &PeerId) {
		self.sync.write().unwrap().on_peer_aborting(&mut NetSyncIo::new(io, self.chain.deref()), *peer);
	}

	fn timeout(&self, io: &NetworkContext<SyncMessage>, _timer: TimerToken) {
		self.sync.write().unwrap().maintain_peers(&mut NetSyncIo::new(io, self.chain.deref()));
		self.sync.write().unwrap().maintain_sync(&mut NetSyncIo::new(io, self.chain.deref()));
	}
}