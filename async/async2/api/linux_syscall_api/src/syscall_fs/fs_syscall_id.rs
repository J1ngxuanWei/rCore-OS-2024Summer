//! 记录该模块使用到的系统调用 id
//!
//!
//!
#[cfg(any(
    target_arch = "riscv32",
    target_arch = "riscv64",
    target_arch = "aarch64"
))]
numeric_enum_macro::numeric_enum! {
#[repr(usize)]
#[allow(non_camel_case_types)]
#[allow(missing_docs)]
#[derive(Eq, PartialEq, Debug, Copy, Clone)]
pub enum FsSyscallId {
    // fs
    OPENAT = 56,
    CLOSE = 57,
    READ = 63,
    WRITE = 64,
}
}

#[cfg(target_arch = "x86_64")]
numeric_enum_macro::numeric_enum! {
    #[repr(usize)]
    #[allow(non_camel_case_types)]
    #[allow(missing_docs)]
    #[derive(Eq, PartialEq, Debug, Copy, Clone)]
    pub enum FsSyscallId {
        // fs
        OPENAT = 257,
        CLOSE = 3,
        READ = 0,
        WRITE = 1,
    }
}
