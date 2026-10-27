//! Integration tests and benchmarks for `echo`, run inside a real (headless) Godot.
//!
//! Run with `./echo_tests/run.sh` (tests) or `./echo_tests/run.sh bench` (benchmarks).

use godot::{classes::Os, prelude::*};

mod bench;
mod tests;
mod util;

macro_rules! __echo_deny {
    ($($t:tt)*) => {};
}
pub(crate) use __echo_deny;

struct EchoTests;

#[gdextension]
unsafe impl ExtensionLibrary for EchoTests {}

#[derive(GodotClass)]
#[class(init, base = Node)]
struct EchoTestRunner {
    base: Base<Node>,
}

#[godot_api]
impl INode for EchoTestRunner {
    fn ready(&mut self) {
        let args: Vec<String> = Os::singleton()
            .get_cmdline_user_args()
            .as_slice()
            .iter()
            .map(|s| s.to_string())
            .collect();
        let filter = args.iter().find(|a| !a.starts_with("--")).cloned();

        let parent: Gd<Node> = self.to_gd().upcast();
        let ok = if args.iter().any(|a| a == "--bench") {
            bench::run_all(&parent, filter.as_deref());
            true
        } else {
            tests::run_all(&parent, filter.as_deref())
        };
        self.base()
            .get_tree()
            .quit_ex()
            .exit_code(if ok { 0 } else { 1 })
            .done();
    }
}
