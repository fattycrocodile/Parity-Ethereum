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

//! Ethcore crypto.

use bigint::uint::*;
use bigint::hash::*;
use bytes::*;
use secp256k1::{key, Secp256k1};
use rand::os::OsRng;
use sha3::Hashable;
use std::fmt;

/// Secret key for secp256k1 EC operations. 256 bit generic "hash" data.
pub type Secret = H256;
/// Public key for secp256k1 EC operations. 512 bit generic "hash" data.
pub type Public = H512;
/// Signature for secp256k1 EC operations; encodes two 256-bit curve points
/// and a third sign bit. 520 bit generic "hash" data.
pub type Signature = H520;

lazy_static! {
	static ref SECP256K1: Secp256k1 = Secp256k1::new();
}

/// Create a new signature from the R, S and V componenets.
pub fn signature_from_rsv(r: &H256, s: &H256, v: u8) -> Signature {
	let mut ret: Signature = Signature::new();
	(&mut ret[0..32]).copy_from_slice(r);
	(&mut ret[32..64]).copy_from_slice(s);

	ret[64] = v;
	ret
}

/// Convert transaction to R, S and V components.
pub fn signature_to_rsv(s: &Signature) -> (U256, U256, u8) {
	(U256::from(&s.as_slice()[0..32]), U256::from(&s.as_slice()[32..64]), s[64])
}

#[derive(Debug)]
/// Crypto error
pub enum CryptoError {
	/// Invalid secret key
	InvalidSecret,
	/// Invalid public key
	InvalidPublic,
	/// Invalid EC signature
	InvalidSignature,
	/// Invalid AES message
	InvalidMessage,
	/// IO Error
	Io(::std::io::Error),
}

impl fmt::Display for CryptoError {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		let msg = match *self {
			CryptoError::InvalidSecret => "Invalid secret key".into(),
			CryptoError::InvalidPublic => "Invalid public key".into(),
			CryptoError::InvalidSignature => "Invalid EC signature".into(),
			CryptoError::InvalidMessage => "Invalid AES message".into(),
			CryptoError::Io(ref err) => format!("I/O error: {}", err),
		};

		f.write_fmt(format_args!("Crypto error ({})", msg))
	}
}

impl From<::secp256k1::Error> for CryptoError {
	fn from(e: ::secp256k1::Error) -> CryptoError {
		match e {
			::secp256k1::Error::InvalidMessage => CryptoError::InvalidMessage,
			::secp256k1::Error::InvalidPublicKey => CryptoError::InvalidPublic,
			::secp256k1::Error::InvalidSecretKey => CryptoError::InvalidSecret,
			_ => CryptoError::InvalidSignature,
		}
	}
}

impl From<::std::io::Error> for CryptoError {
	fn from(err: ::std::io::Error) -> CryptoError {
		CryptoError::Io(err)
	}
}

#[derive(Debug, PartialEq, Eq)]
/// secp256k1 Key pair
///
/// Use `create()` to create a new random key pair.
///
/// # Example
/// ```rust
/// extern crate ethcore_util;
/// use ethcore_util::crypto::*;
/// use ethcore_util::hash::*;
/// fn main() {
///   let pair = KeyPair::create().unwrap();
///   let message = H256::random();
///   let signature = ec::sign(pair.secret(), &message).unwrap();
///
///   assert!(ec::verify(pair.public(), &signature, &message).unwrap());
///   assert_eq!(ec::recover(&signature, &message).unwrap(), *pair.public());
/// }
/// ```
pub struct KeyPair {
	secret: Secret,
	public: Public,
}

impl KeyPair {
	/// Create a pair from secret key
	pub fn from_secret(secret: Secret) -> Result<KeyPair, CryptoError> {
		let context = &SECP256K1;
		let s: key::SecretKey = try!(key::SecretKey::from_slice(context, &secret));
		let pub_key = try!(key::PublicKey::from_secret_key(context, &s));
		let serialized = pub_key.serialize_vec(context, false);
		let p: Public = Public::from_slice(&serialized[1..65]);
		Ok(KeyPair {
			secret: secret,
			public: p,
		})
	}


	// TODO: move to ethstore/secret.rs once @debris has refactored necessary dependencies into own crate
	/// Convert the given phrase into a secret as per brain-wallet spec. 
	/// Taken from https://github.com/ethereum/wiki/wiki/Brain-Wallet
	/// Note particularly secure for low-entropy keys.
	pub fn from_phrase(phrase: &str) -> KeyPair {
		let mut h = phrase.as_bytes().sha3();
		for _ in 0..16384 {
			h = h.sha3();
		}
		loop {
			let r = KeyPair::from_secret(h);
			if r.is_ok() {
				let r = r.unwrap(); 
				if r.address()[0] == 0 {
					return r;
				}
			}
			h = h.sha3();
		}
	}
	
	/// Create a new random key pair
	pub fn create() -> Result<KeyPair, CryptoError> {
		let context = &SECP256K1;
		let mut rng = try!(OsRng::new());
		let (sec, publ) = try!(context.generate_keypair(&mut rng));
		let serialized = publ.serialize_vec(context, false);
		let p: Public = Public::from_slice(&serialized[1..65]);

		let mut s = Secret::new();
		s.copy_from_slice(&sec[0..32]);

		Ok(KeyPair {
			secret: s,
			public: p,
		})
	}

	/// Returns public key
	pub fn public(&self) -> &Public {
		&self.public
	}

	/// Returns private key
	pub fn secret(&self) -> &Secret {
		&self.secret
	}

	/// Returns address.
	pub fn address(&self) -> Address {
		Address::from(self.public.sha3())
	}

	/// Sign a message with our secret key.
	pub fn sign(&self, message: &H256) -> Result<Signature, CryptoError> { ec::sign(&self.secret, message) }
}

/// EC functions
#[cfg_attr(feature="dev", allow(similar_names))]
pub mod ec {
	use bigint::hash::*;
	use bigint::uint::*;
	use standard::*;
	use crypto::*;
	use crypto::{self};

	/// Recovers Public key from signed message hash.
	pub fn recover(signature: &Signature, message: &H256) -> Result<Public, CryptoError> {
		use secp256k1::*;
		let context = &crypto::SECP256K1;
		let rsig = try!(RecoverableSignature::from_compact(context, &signature[0..64], try!(RecoveryId::from_i32(signature[64] as i32))));
		let publ = try!(context.recover(&try!(Message::from_slice(&message)), &rsig));
		let serialized = publ.serialize_vec(context, false);
		let p: Public = Public::from_slice(&serialized[1..65]);
		//TODO: check if it's the zero key and fail if so.
		Ok(p)
	}
	/// Returns siganture of message hash.
	pub fn sign(secret: &Secret, message: &H256) -> Result<Signature, CryptoError> {
		// TODO: allow creation of only low-s signatures.
		use secp256k1::{Message, key};

		let context = &crypto::SECP256K1;
		// no way to create from raw byte array.
		let sec: &key::SecretKey = unsafe { ::std::mem::transmute(secret) };
		let s = try!(context.sign_recoverable(&try!(Message::from_slice(&message)), sec));
		let (rec_id, data) = s.serialize_compact(context);
		let mut signature = crypto::Signature::new();
		signature.clone_from_slice(&data);
		signature[64] = rec_id.to_i32() as u8;

		let (_, s, v) = signature_to_rsv(&signature);
		let secp256k1n = U256::from_str("fffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141").unwrap();
		if !is_low_s(&s) {
			signature = super::signature_from_rsv(&H256::from_slice(&signature[0..32]), &H256::from(secp256k1n - s), v ^ 1);
		}
		Ok(signature)
	}

	/// Verify signature.
	pub fn verify(public: &Public, signature: &Signature, message: &H256) -> Result<bool, CryptoError> {
		use secp256k1::*;
		let context = &crypto::SECP256K1;
		let rsig = try!(RecoverableSignature::from_compact(context, &signature[0..64], try!(RecoveryId::from_i32(signature[64] as i32))));
		let sig = rsig.to_standard(context);

		let pdata: [u8; 65] = {
			let mut temp = [4u8; 65];
			(&mut temp[1..65]).copy_from_slice(public);
			temp
		};

		let publ = try!(key::PublicKey::from_slice(context, &pdata));
		match context.verify(&try!(Message::from_slice(&message)), &sig, &publ) {
			Ok(_) => Ok(true),
			Err(Error::IncorrectSignature) => Ok(false),
			Err(x) => Err(CryptoError::from(x))
		}
	}

	/// Check if this is a "low" signature.
	pub fn is_low(sig: &Signature) -> bool {
		H256::from_slice(&sig[32..64]) <= "7fffffffffffffffffffffffffffffff5d576e7357a4501ddfe92f46681b20a0".into()
	}

	/// Check if this is a "low" signature.
	pub fn is_low_s(s: &U256) -> bool {
		s <= &U256::from_str("7FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF5D576E7357A4501DDFE92F46681B20A0").unwrap()
	}

	/// Check if each component of the signature is in range.
	pub fn is_valid(sig: &Signature) -> bool {
		sig[64] <= 1 &&
			H256::from_slice(&sig[0..32]) < "fffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141".into() &&
			H256::from_slice(&sig[32..64]) < "fffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141".into() &&
			H256::from_slice(&sig[32..64]) >= 1.into() &&
			H256::from_slice(&sig[0..32]) >= 1.into()
	}
}

/// ECDH functions
#[cfg_attr(feature="dev", allow(similar_names))]
pub mod ecdh {
	use hash::FixedHash;
	use crypto::{self, Secret, Public, CryptoError};

	/// Agree on a shared secret
	pub fn agree(secret: &Secret, public: &Public) -> Result<Secret, CryptoError> {
		use secp256k1::{ecdh, key};

		let context = &crypto::SECP256K1;
		let pdata = {
			let mut temp = [4u8; 65];
			(&mut temp[1..65]).copy_from_slice(&public[0..64]);
			temp
		};

		let publ = try!(key::PublicKey::from_slice(context, &pdata));
		// no way to create SecretKey from raw byte array.
		let sec: &key::SecretKey = unsafe { ::std::mem::transmute(secret) };
		let shared = ecdh::SharedSecret::new_raw(context, &publ, sec);

		let mut s = crypto::Secret::new();
		s.copy_from_slice(&shared[0..32]);
		Ok(s)
	}
}

/// ECIES function
#[cfg_attr(feature="dev", allow(similar_names))]
pub mod ecies {
	use hash::*;
	use bytes::*;
	use crypto::*;

	/// Encrypt a message with a public key
	pub fn encrypt(public: &Public, shared_mac: &[u8], plain: &[u8]) -> Result<Bytes, CryptoError> {
		use ::rcrypto::digest::Digest;
		use ::rcrypto::sha2::Sha256;
		use ::rcrypto::hmac::Hmac;
		use ::rcrypto::mac::Mac;
		let r = try!(KeyPair::create());
		let z = try!(ecdh::agree(r.secret(), public));
		let mut key = [0u8; 32];
		let mut mkey = [0u8; 32];
		kdf(&z, &[0u8; 0], &mut key);
		let mut hasher = Sha256::new();
		let mkey_material = &key[16..32];
		hasher.input(mkey_material);
		hasher.result(&mut mkey);
		let ekey = &key[0..16];

		let mut msg = vec![0u8; (1 + 64 + 16 + plain.len() + 32)];
		msg[0] = 0x04u8;
		{
			let msgd = &mut msg[1..];
			r.public().copy_to(&mut msgd[0..64]);
			{
				let cipher = &mut msgd[(64 + 16)..(64 + 16 + plain.len())];
				aes::encrypt(ekey, &H128::new(), plain, cipher);
			}
			let mut hmac = Hmac::new(Sha256::new(), &mkey);
			{
				let cipher_iv = &msgd[64..(64 + 16 + plain.len())];
				hmac.input(cipher_iv);
			}
			hmac.input(shared_mac);
			hmac.raw_result(&mut msgd[(64 + 16 + plain.len())..]);
		}
		Ok(msg)
	}

	/// Decrypt a message with a secret key
	pub fn decrypt(secret: &Secret, shared_mac: &[u8], encrypted: &[u8]) -> Result<Bytes, CryptoError> {
		use ::rcrypto::digest::Digest;
		use ::rcrypto::sha2::Sha256;
		use ::rcrypto::hmac::Hmac;
		use ::rcrypto::mac::Mac;

		let meta_len = 1 + 64 + 16 + 32;
		if encrypted.len() < meta_len  || encrypted[0] < 2 || encrypted[0] > 4 {
			return Err(CryptoError::InvalidMessage); //invalid message: publickey
		}

		let e = &encrypted[1..];
		let p = Public::from_slice(&e[0..64]);
		let z = try!(ecdh::agree(secret, &p));
		let mut key = [0u8; 32];
		kdf(&z, &[0u8; 0], &mut key);
		let ekey = &key[0..16];
		let mkey_material = &key[16..32];
		let mut hasher = Sha256::new();
		let mut mkey = [0u8; 32];
		hasher.input(mkey_material);
		hasher.result(&mut mkey);

		let clen = encrypted.len() - meta_len;
		let cipher_with_iv = &e[64..(64+16+clen)];
		let cipher_iv = &cipher_with_iv[0..16];
		let cipher_no_iv = &cipher_with_iv[16..];
		let msg_mac = &e[(64+16+clen)..];

		// Verify tag
		let mut hmac = Hmac::new(Sha256::new(), &mkey);
		hmac.input(cipher_with_iv);
		hmac.input(shared_mac);
		let mut mac = H256::new();
		hmac.raw_result(&mut mac);
		if &mac[..] != msg_mac {
			return Err(CryptoError::InvalidMessage);
		}

		let mut msg = vec![0u8; clen];
		aes::decrypt(ekey, cipher_iv, cipher_no_iv, &mut msg[..]);
		Ok(msg)
	}

	fn kdf(secret: &Secret, s1: &[u8], dest: &mut [u8]) {
		use ::rcrypto::digest::Digest;
		use ::rcrypto::sha2::Sha256;
		let mut hasher = Sha256::new();
		// SEC/ISO/Shoup specify counter size SHOULD be equivalent
		// to size of hash output, however, it also notes that
		// the 4 bytes is okay. NIST specifies 4 bytes.
		let mut ctr = 1u32;
		let mut written = 0usize;
		while written < dest.len() {
			let ctrs = [(ctr >> 24) as u8, (ctr >> 16) as u8, (ctr >> 8) as u8, ctr as u8];
			hasher.input(&ctrs);
			hasher.input(secret);
			hasher.input(s1);
			hasher.result(&mut dest[written..(written + 32)]);
			hasher.reset();
			written += 32;
			ctr += 1;
		}
	}
}

/// AES encryption
pub mod aes {
	use ::rcrypto::blockmodes::*;
	use ::rcrypto::aessafe::*;
	use ::rcrypto::symmetriccipher::*;
	use ::rcrypto::buffer::*;

	/// Encrypt a message
	pub fn encrypt(k: &[u8], iv: &[u8], plain: &[u8], dest: &mut [u8]) {
		let mut encryptor = CtrMode::new(AesSafe128Encryptor::new(k), iv.to_vec());
		encryptor.encrypt(&mut RefReadBuffer::new(plain), &mut RefWriteBuffer::new(dest), true).expect("Invalid length or padding");
	}

	/// Decrypt a message
	pub fn decrypt(k: &[u8], iv: &[u8], encrypted: &[u8], dest: &mut [u8]) {
		let mut encryptor = CtrMode::new(AesSafe128Encryptor::new(k), iv.to_vec());
		encryptor.decrypt(&mut RefReadBuffer::new(encrypted), &mut RefWriteBuffer::new(dest), true).expect("Invalid length or padding");
	}
}


#[cfg(test)]
mod tests {
	use hash::*;
	use crypto::*;

	// TODO: tests for sign/recover roundtrip, at least.

	#[test]
	fn test_signature() {
		let pair = KeyPair::create().unwrap();
		let message = H256::random();
		let signature = ec::sign(pair.secret(), &message).unwrap();

		assert!(ec::verify(pair.public(), &signature, &message).unwrap());
		assert_eq!(ec::recover(&signature, &message).unwrap(), *pair.public());
	}

	#[test]
	fn test_invalid_key() {
		assert!(KeyPair::from_secret("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".into()).is_err());
		assert!(KeyPair::from_secret("0000000000000000000000000000000000000000000000000000000000000000".into()).is_err());
		assert!(KeyPair::from_secret("fffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141".into()).is_err());
	}

	#[test]
	fn test_key() {
		let pair = KeyPair::from_secret("6f7b0d801bc7b5ce7bbd930b84fd0369b3eb25d09be58d64ba811091046f3aa2".into()).unwrap();
		assert_eq!(pair.public().hex(), "101b3ef5a4ea7a1c7928e24c4c75fd053c235d7b80c22ae5c03d145d0ac7396e2a4ffff9adee3133a7b05044a5cee08115fd65145e5165d646bde371010d803c");
	}

	#[test]
	fn test_key_from_phrase() {
		assert_eq!(KeyPair::from_phrase("correct horse battery staple").address(), "0021f80b7f29b9c84e8099c2c6c74a46ed2268c4".into());
	}

	#[test]
	fn ecies_shared() {
		let kp = KeyPair::create().unwrap();
		let message = b"So many books, so little time";

		let shared = b"shared";
		let wrong_shared = b"incorrect";
		let encrypted = ecies::encrypt(kp.public(), shared, message).unwrap();
		assert!(encrypted[..] != message[..]);
		assert_eq!(encrypted[0], 0x04);

		assert!(ecies::decrypt(kp.secret(), wrong_shared, &encrypted).is_err());
		let decrypted = ecies::decrypt(kp.secret(), shared, &encrypted).unwrap();
		assert_eq!(decrypted[..message.len()], message[..]);
	}
}
