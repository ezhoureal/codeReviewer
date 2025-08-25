mod code_reviewer;
use crate::code_reviewer::CodeReviewer;

use std::env;
use std::path::Path;
use anyhow::Result;
use clap::{Arg, Command as ClapCommand};


#[tokio::main]
async fn main() -> Result<()> {
    let matches = ClapCommand::new("Code Reviewer")
        .version("1.0")
        .about("Analyzes git changes for potential breaking changes using AI")
        .arg(
            Arg::new("directory")
                .help("Path to the git repository to analyze")
                .required(true)
                .index(1)
        )
        .get_matches();

    let repo_path = matches.get_one::<String>("directory").unwrap().clone();

    // Validate directory exists
    if !Path::new(&repo_path).is_dir() {
        anyhow::bail!("Error: '{}' is not a valid directory", repo_path);
    }

    // Check if API key is set
    if env::var("MOONSHOT_API_KEY").is_err() {
        eprintln!("❌ Error: MOONSHOT_API_KEY environment variable is not set.");
        eprintln!("Please set it with: export MOONSHOT_API_KEY=your_api_key_here");
        std::process::exit(1);
    }

    println!("🚀 Starting code review for: {}", repo_path);
    
    let reviewer = CodeReviewer::new(repo_path)?;
    
    if let Err(e) = reviewer.review_changes().await {
        eprintln!("❌ Error: {}", e);
        std::process::exit(1);
    }

    println!("\n✅ Code review completed!");
    Ok(())
}
