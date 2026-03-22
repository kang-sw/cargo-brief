//! Test fixture crate for cargo-brief integration tests.
//!
//! This crate exercises all supported item types.

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

    /// A trait with an associated type.
    pub trait Converter {
        type Output;
        fn convert(&self) -> Self::Output;
    }

    impl Converter for PubStruct {
        type Output = String;
        fn convert(&self) -> String {
            format!("{}", self.pub_field)
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

    // --- Where clauses ---

    /// A function with where clause bounds.
    pub fn where_fn<T, U>(a: T, b: U) -> String
    where
        T: std::fmt::Display + Clone,
        U: Into<String>,
    {
        format!("{}{}", a, b.into())
    }

    /// A function with multiple where clause bounds.
    pub fn multi_where<T, U, V>(a: T, b: U, c: V) -> String
    where
        T: std::fmt::Display,
        U: std::fmt::Debug + Clone,
        V: Into<String> + Send,
    {
        format!("{a}{b:?}{}", c.into())
    }

    /// A function with lifetime where clause.
    pub fn lifetime_where<'a, 'b>(a: &'a str, b: &'b str) -> &'a str
    where
        'b: 'a,
    {
        if a.len() > b.len() { a } else { a }
    }

    /// A struct with where clause.
    pub struct WhereStruct<T>
    where
        T: std::fmt::Debug,
    {
        pub value: T,
    }

    impl<T> WhereStruct<T>
    where
        T: std::fmt::Debug + Clone,
    {
        pub fn get_value(&self) -> &T {
            &self.value
        }
    }

    /// A trait with where clause.
    pub trait WhereTrait<T>
    where
        T: Clone,
    {
        fn apply(&self, val: T) -> T;
    }

    /// A type alias with where clause.
    pub type WhereAlias<T>
    where
        T: Ord,
    = Vec<T>;

    // --- impl Trait syntax ---

    /// A function with impl Trait argument.
    pub fn impl_trait_fn(val: impl std::fmt::Display) -> String {
        format!("{val}")
    }

    /// A function with multiple impl Trait arguments.
    pub fn multi_impl_trait(a: impl std::fmt::Display, b: impl std::fmt::Debug) -> String {
        format!("{a}{b:?}")
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

    // --- Derived traits ---

    /// A struct with many derived traits for testing trait impl collapsing.
    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    pub struct DerivedStruct {
        pub value: i32,
    }

    impl std::fmt::Display for DerivedStruct {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.value)
        }
    }

    // --- Private module with pub items, glob-re-exported (nested) ---
    // Mirrors bevy's `mod bind_group; pub use bind_group::*;` pattern
    mod nested_private {
        pub struct NestedPrivateStruct {
            pub field: i32,
        }

        pub trait NestedPrivateTrait {
            fn nested_method(&self);
        }
    }

    pub use nested_private::*;
}

// --- Deprecated / Non-exhaustive test items ---

#[deprecated(since = "0.1.0", note = "use new_function instead")]
pub fn deprecated_function() -> bool {
    true
}

#[deprecated = "old struct"]
pub struct DeprecatedStruct;

#[non_exhaustive]
pub enum NonExhaustiveEnum {
    A,
    B,
}

pub use outer::PubStruct as ReExported;

// --- Glob re-export from pub(crate) module ---

pub(crate) mod hidden_reexport {
    /// A trait only accessible via glob re-export.
    pub trait GlobTrait {
        fn glob_method(&self) -> bool;
    }

    /// A struct only accessible via glob re-export.
    pub struct GlobStruct {
        pub visible_field: i32,
    }

    impl GlobStruct {
        pub fn glob_fn(&self) -> i32 {
            self.visible_field
        }
    }
}

pub use hidden_reexport::*;

// --- Cross-crate glob re-export chain ---
// test-fixture → glob-source → glob-inner (2-level chain)
pub use glob_source::*;

// --- Named cross-crate re-exports (NOT via glob) ---
pub use named_source::NamedSourceItem;
pub use named_source::NamedSourceTrait;
