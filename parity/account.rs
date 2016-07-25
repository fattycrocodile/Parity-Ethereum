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

use ethcore::ethstore::{EthStore, import_accounts};
use ethcore::ethstore::dir::DiskDirectory;
use ethcore::account_provider::AccountProvider;
use helpers::{password_prompt, password_from_file};

#[derive(Debug, PartialEq)]
pub enum AccountCmd {
	New(NewAccount),
	List(String),
	Import(ImportAccounts),
}

#[derive(Debug, PartialEq)]
pub struct NewAccount {
	pub iterations: u32,
	pub path: String,
	pub password_file: Option<String>,
}

#[derive(Debug, PartialEq)]
pub struct ImportAccounts {
	pub from: Vec<String>,
	pub to: String,
}

pub fn execute(cmd: AccountCmd) -> Result<String, String> {
	match cmd {
		AccountCmd::New(new_cmd) => new(new_cmd),
		AccountCmd::List(path) => list(path),
		AccountCmd::Import(import_cmd) => import(import_cmd),
	}
}

fn new(n: NewAccount) -> Result<String, String> {
	let password: String = match n.password_file {
		Some(file) => try!(password_from_file(file)),
		None => try!(password_prompt()),
	};

	let dir = Box::new(DiskDirectory::create(n.path).unwrap());
	let secret_store = Box::new(EthStore::open_with_iterations(dir, n.iterations).unwrap());
	let acc_provider = AccountProvider::new(secret_store);
	let new_account = acc_provider.new_account(&password).unwrap();
	Ok(format!("{:?}", new_account))
}

fn list(path: String) -> Result<String, String> {
	let dir = Box::new(DiskDirectory::create(path).unwrap());
	let secret_store = Box::new(EthStore::open(dir).unwrap());
	let acc_provider = AccountProvider::new(secret_store);
	let accounts = acc_provider.accounts();
	let result = accounts.into_iter()
		.map(|a| format!("{:?}", a))
		.collect::<Vec<String>>()
		.join("\n");

	Ok(result)
}

fn import(i: ImportAccounts) -> Result<String, String> {
	let to = DiskDirectory::create(i.to).unwrap();
	let mut imported = 0;
	for path in &i.from {
		let from = DiskDirectory::at(path);
		imported += try!(import_accounts(&from, &to).map_err(|_| "Importing accounts failed.")).len();
	}
	Ok(format!("{}", imported))
}
