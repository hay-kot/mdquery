use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
#[command(
    name = "mdquery",
    version,
    about = "SQL queries for markdown frontmatter"
)]
struct Cli {
    /// SQL query (e.g. "SELECT title, date FROM . WHERE status = 'draft'")
    query: String,

    /// Output format
    #[arg(short, long, default_value = "table")]
    format: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let format: mdquery::output::Format = cli.format.parse()?;
    let start = std::time::Instant::now();
    let output = mdquery::run(&cli.query, format)?;
    let elapsed = start.elapsed();
    println!("{output}");
    eprintln!("Query completed in {elapsed:.2?}");
    Ok(())
}
