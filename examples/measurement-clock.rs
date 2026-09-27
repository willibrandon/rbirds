//! Cross-process clock qualification for the performance harness.
use std::io::{self, BufRead, Write};

fn main() -> io::Result<()> {
    let mut output = io::stdout().lock();
    writeln!(output, "{}", rbirds::platform::MEASUREMENT_CLOCK)?;
    output.flush()?;
    for line in io::stdin().lock().lines() {
        line?;
        writeln!(output, "{}", rbirds::platform::measurement_clock_ns()?)?;
        output.flush()?;
    }
    Ok(())
}
