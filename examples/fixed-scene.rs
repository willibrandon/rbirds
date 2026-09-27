//! Fixed simulation steps for CPU comparisons. Not a presentation benchmark.

fn main() -> std::process::ExitCode {
    rbirds::platform::default_sigpipe();
    let mut output = rbirds::stdio::CStdout::new();
    let mut program = rbirds::app::Program::new();
    let code = match rbirds::app::read_options(
        &mut program,
        &std::env::args_os().collect::<Vec<_>>(),
        &mut output,
    ) {
        Ok(()) => rbirds::live::run_fixed_scene(&mut program, &mut output),
        Err(code) => code,
    };
    output.flush();
    std::process::ExitCode::from(code as u8)
}
