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
#![cfg_attr(all(nightly, feature="dev"), feature(plugin))]
#![cfg_attr(all(nightly, feature="dev"), plugin(clippy))]

//! Signer module
//!
//! This module manages your private keys and accounts/identities
//! that can be used within Dapps.
//!
//! It exposes API (over `WebSockets`) accessed by System UIs.
//! Each transaction sent by Dapp is broadcasted to System UIs
//! and their responsibility is to confirm (or confirm and sign)
//! the transaction for you.
//!
//! ```
//! extern crate ethcore_signer;
//!
//! use ethcore_signer::ServerBuilder;
//!
//!	fn main() {
//!	 let builder = ServerBuilder::new();
//!	 let _server = builder.start("127.0.0.1:8084".parse().unwrap()).unwrap();
//!	}
//! ```

#[macro_use]
extern crate log;
extern crate env_logger;

extern crate ethcore_util as util;
extern crate ethcore_rpc as rpc;
extern crate jsonrpc_core;
extern crate ws;

mod ws_server;
pub use ws_server::*;

#[cfg(test)]
mod tests {
	#[test]
	fn should_work() {}
}
