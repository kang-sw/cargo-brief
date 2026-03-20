//! Example demonstrating basic usage of the test fixture crate.
//!
//! This file exists for integration testing of cargo-brief's examples subcommand.

use test_fixture::outer::PubStruct;

fn main() {
    let s = PubStruct {
        pub_field: 42,
        crate_field: 0,
        super_field: 0,
        private_field: 0,
    };
    println!("value: {}", s.pub_method());
}
