pub mod outer {
    pub struct PubStruct {
        pub pub_field: i32,
        pub(crate) crate_field: i32,
        pub(super) super_field: i32,
        private_field: i32,
    }

    impl PubStruct {
        pub fn pub_method(&self) -> i32 {
            self.pub_field
        }
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
        fn do_thing(&self) -> bool {
            true
        }
    }

    pub type Alias = PubStruct;

    pub const MY_CONST: i32 = 42;

    // --- Enums ---

    /// A plain enum (C-like).
    pub enum PlainEnum {
        /// First variant.
        Alpha,
        Beta,
        Gamma,
    }

    /// An enum with tuple variants.
    pub enum TupleEnum {
        One(i32),
        Two(String, bool),
        Empty,
    }

    /// An enum with struct variants.
    pub enum StructEnum {
        Point { x: f64, y: f64 },
        Named { name: String, value: i32 },
    }

    // --- Free functions ---

    /// A regular public function.
    pub fn free_function(x: i32, y: i32) -> i32 {
        x + y
    }

    /// An async function.
    pub async fn async_function() -> String {
        String::new()
    }

    /// A const function.
    pub const fn const_function(x: u32) -> u32 {
        x * 2
    }

    /// An unsafe function.
    pub unsafe fn unsafe_function(ptr: *const u8) -> u8 {
        unsafe { *ptr }
    }

    // --- Generics ---

    /// A generic struct.
    pub struct GenericStruct<T: Clone, U = ()> {
        pub value: T,
        pub extra: U,
    }

    /// A generic trait with bounds.
    pub trait GenericTrait<T: Send + Sync>: Clone {
        type Output;
        fn process(&self, input: T) -> Self::Output;
    }

    /// A generic function.
    pub fn generic_function<T: std::fmt::Debug + Clone>(items: &[T]) -> Vec<T> {
        items.to_vec()
    }

    // --- Macros ---

    /// A declarative macro.
    #[macro_export]
    macro_rules! my_macro {
        ($x:expr) => {
            $x + 1
        };
    }

    // --- Statics ---

    /// A static variable.
    pub static GLOBAL_COUNT: std::sync::atomic::AtomicU32 =
        std::sync::atomic::AtomicU32::new(0);

    /// A mutable static.
    pub static mut MUTABLE_GLOBAL: i32 = 0;

    // --- Union ---

    /// A union type.
    #[repr(C)]
    pub union MyUnion {
        pub int_val: i32,
        pub float_val: f32,
    }
}

pub use outer::PubStruct as ReExported;
