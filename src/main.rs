use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use clap::Parser;

use procrun::dotenv::parse_dotenv;
use procrun::procfile::parse_procfile;
use procrun::runner::run;

#[derive(Parser)]
#[command(
    name = "procrun",
    about = "Runs a Procfile — foreman/overmind for people who don't want Ruby or tmux"
)]
struct Cli {
    /// Path to the Procfile.
    #[arg(default_value = "Procfile")]
    procfile: PathBuf,

    /// Load environment variables from a `.env`-style file into every process.
    #[arg(short = 'e', long)]
    env: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let procfile_text = fs::read_to_string(&cli.procfile)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", cli.procfile.display()))?;
    let entries = parse_procfile(&procfile_text).map_err(|e| anyhow::anyhow!(e))?;

    let extra_env: HashMap<String, String> = match &cli.env {
        Some(path) => {
            let text = fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
            parse_dotenv(&text)
        }
        None => HashMap::new(),
    };

    println!(
        "procrun: starting {} process(es) from {}",
        entries.len(),
        cli.procfile.display()
    );
    let code = run(entries, &extra_env);
    std::process::exit(code);
}
