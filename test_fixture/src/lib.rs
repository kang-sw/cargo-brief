pub mod outer {
    pub struct PubStruct {
        pub pub_field: i32,
        pub(crate) crate_field: i32,
        pub(super) super_field: i32,
        private_field: i32,
    }

    impl PubStruct {
        pub fn pub_method(&self) -> i32 { self.pub_field }
        pub(crate) fn crate_method(&self) {}
        fn private_method(&self) {}
    }

    pub(crate) struct CrateStruct;
    pub(super) struct SuperStruct;
    struct PrivateStruct;

    pub mod inner {
        pub struct InnerPub;
        pub(crate) struct InnerCrate;
        pub(super) struct InnerSuper;
        pub(in crate::outer) struct InnerRestricted;
    }

    /// A documented trait.
    pub trait MyTrait {
        /// Trait method.
        fn do_thing(&self) -> bool;
    }

    impl MyTrait for PubStruct {
        fn do_thing(&self) -> bool { true }
    }

    pub type Alias = PubStruct;

    pub const MY_CONST: i32 = 42;
}

pub use outer::PubStruct as ReExported;
