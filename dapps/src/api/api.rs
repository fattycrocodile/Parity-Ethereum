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

use std::sync::Arc;
use endpoint::{Endpoint, Endpoints, EndpointInfo, Handler, EndpointPath};

use api::response::as_json;

pub struct RestApi {
	endpoints: Arc<Endpoints>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct App {
	pub id: String,
	pub name: String,
	pub description: String,
	pub version: String,
	pub author: String,
	#[serde(rename="iconUrl")]
	pub icon_url: String,
}

impl App {
	fn from_info(id: &str, info: &EndpointInfo) -> Self {
		App {
			id: id.to_owned(),
			name: info.name.to_owned(),
			description: info.description.to_owned(),
			version: info.version.to_owned(),
			author: info.author.to_owned(),
			icon_url: info.icon_url.to_owned(),
		}
	}
}

impl RestApi {
	pub fn new(endpoints: Arc<Endpoints>) -> Box<Endpoint> {
		Box::new(RestApi {
			endpoints: endpoints
		})
	}

	fn list_apps(&self) -> Vec<App> {
		self.endpoints.iter().filter_map(|(ref k, ref e)| {
			e.info().map(|ref info| App::from_info(k, info))
		}).collect()
	}
}

impl Endpoint for RestApi {
	fn to_handler(&self, _path: EndpointPath) -> Box<Handler> {
		as_json(&self.list_apps())
	}
}

