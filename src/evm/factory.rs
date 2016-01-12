//! Evm factory.

use evm::Evm;

/// Evm factory. Creates appropriate Evm.
pub struct Factory;

impl Factory {
	/// Returns jit vm
	#[cfg(feature = "jit")]
	pub fn create() -> Box<Evm> {
		Box::new(super::jit::JitEvm)
	}

	/// Returns native rust evm
	#[cfg(not(feature = "jit"))]
	pub fn create() -> Box<Evm> {
		Box::new(super::interpreter::Interpreter::new())
	}
}

#[test]
fn test_create_vm() {
	let _vm = Factory::create();
}
