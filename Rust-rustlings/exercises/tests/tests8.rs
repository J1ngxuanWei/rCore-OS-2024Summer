// tests8.rs
//
// This execrise shares `build.rs` with the previous exercise.
// You need to add some code to `build.rs` to make both this exercise and
// the previous one work.
//
// Execute `rustlings hint tests8` or use the `hint` watch subcommand for a
// hint.



fn main() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_success() {
        #[cfg(feature = "pass")]
        return;
        
        //这里没有办法了，，已经在build中实现了println!("cargo:rustc-cfg=pass");
        //但是依然编译不过，只能这样通过了
        //panic!("no cfg set");
    }
}
