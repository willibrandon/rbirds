//! The `rbirds` executable: everything happens in [`rbirds::app::main`],
//! which returns only after the terminal has been put back.

#![forbid(unsafe_code)]

fn main() {
    let code = rbirds::app::main(std::env::args_os().collect());
    std::process::exit(code);
}
