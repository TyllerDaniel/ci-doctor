use ci_doctor::{InputFormatArg, diagnose, report};
use clap::{Parser, Subcommand, ValueEnum};
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

/// Classify CI failures from a raw log or JUnit XML and suggest what to do next.
///
/// Exit codes: 0 no failures found, 1 failures found, 2 usage or input error.
#[derive(Parser)]
#[command(name = "ci-doctor", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Diagnose a CI log or JUnit XML report.
    Diagnose {
        /// File to read, or `-` for stdin.
        #[arg(default_value = "-")]
        input: PathBuf,
        /// Output format.
        #[arg(long, value_enum, default_value_t = Output::Text)]
        format: Output,
        /// How to parse the input. `auto` treats XML starting with <testsuite as JUnit.
        #[arg(long, value_enum, default_value_t = Input::Auto)]
        input_format: Input,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Output {
    Text,
    Json,
}

#[derive(Clone, Copy, ValueEnum)]
enum Input {
    Auto,
    Junit,
    Text,
}

fn read_input(path: &PathBuf) -> std::io::Result<String> {
    let bytes = if path.as_os_str() == "-" {
        let mut buf = Vec::new();
        std::io::stdin().read_to_end(&mut buf)?;
        buf
    } else {
        std::fs::read(path)?
    };
    // CI logs are not always valid UTF-8; don't fail on a stray byte.
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let Command::Diagnose {
        input,
        format,
        input_format,
    } = cli.command;

    let text = match read_input(&input) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("ci-doctor: cannot read {}: {e}", input.display());
            return ExitCode::from(2);
        }
    };
    let arg = match input_format {
        Input::Auto => InputFormatArg::Auto,
        Input::Junit => InputFormatArg::Junit,
        Input::Text => InputFormatArg::Text,
    };
    let diagnosis = match diagnose(&text, arg) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("ci-doctor: {e}");
            return ExitCode::from(2);
        }
    };

    match format {
        Output::Json => println!(
            "{}",
            serde_json::to_string_pretty(&diagnosis).expect("diagnosis serializes")
        ),
        Output::Text => print!("{}", report::render_text(&diagnosis)),
    }
    if diagnosis.total_failures > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
