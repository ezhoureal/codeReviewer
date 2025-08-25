use std::env;
use std::path::Path;
use std::process::Command;
use reqwest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio;
use anyhow::{Result, Context};
use clap::{Arg, Command as ClapCommand};

#[derive(Debug, Serialize, Deserialize)]
struct KimiResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Debug, Serialize, Deserialize)]
struct Message {
    content: String,
}

struct GitDiff {
    file_path: String,
    content: String,
}

struct CodeReviewer {
    api_key: String,
    repo_path: String,
}

impl CodeReviewer {
    fn new(repo_path: String) -> Result<Self> {
        let api_key = env::var("MOONSHOT_API_KEY")
            .context("MOONSHOT_API_KEY environment variable not set")?;
        
        Ok(CodeReviewer {
            api_key,
            repo_path,
        })
    }

    fn validate_git_repository(&self) -> Result<()> {
        let output = Command::new("git")
            .arg("status")
            .current_dir(&self.repo_path)
            .output()
            .context("Failed to execute git status. Make sure git is installed and the directory is a git repository")?;

        if !output.status.success() {
            anyhow::bail!("The directory '{}' is not a git repository", self.repo_path);
        }

        Ok(())
    }

    fn get_unstaged_changes(&self) -> Result<Vec<GitDiff>> {
        // Get list of modified files
        let output = Command::new("git")
            .args(&["diff", "--name-only"])
            .current_dir(&self.repo_path)
            .output()
            .context("Failed to get list of modified files")?;

        if !output.status.success() {
            anyhow::bail!("Failed to get git diff: {}", String::from_utf8_lossy(&output.stderr));
        }

        let files = String::from_utf8(output.stdout)?;
        let mut diffs = Vec::new();

        for file_path in files.lines() {
            if file_path.trim().is_empty() {
                continue;
            }

            // Get the actual diff for this file
            let diff_output = Command::new("git")
                .args(&["diff", file_path])
                .current_dir(&self.repo_path)
                .output()
                .context(format!("Failed to get diff for file: {}", file_path))?;

            if diff_output.status.success() {
                let diff_content = String::from_utf8(diff_output.stdout)?;
                if !diff_content.trim().is_empty() {
                    diffs.push(GitDiff {
                        file_path: file_path.to_string(),
                        content: diff_content,
                    });
                }
            }
        }

        Ok(diffs)
    }

    async fn analyze_with_kimi(&self, diffs: &[GitDiff]) -> Result<String> {
        let client = reqwest::Client::new();
        
        // Prepare the prompt for the LLM
        let mut prompt = String::from(
            "You are a senior code reviewer. Analyze the following git diffs to identify potential breaking changes that could affect the behavior of the software. \
            For each change, determine:\n\
            1. Whether it's a breaking change (yes/no)\n\
            2. The severity (low/medium/high)\n\
            3. What behavior might be affected\n\
            4. Suggestions to prevent or mitigate the breaking change\n\n\
            Please provide a structured analysis in the following format:\n\
            ## Summary\n\
            [Overall assessment]\n\n\
            ## Detailed Analysis\n\
            ### File: [filename]\n\
            - **Breaking Change**: [yes/no]\n\
            - **Severity**: [low/medium/high]\n\
            - **Impact**: [description of what might break]\n\
            - **Suggestions**: [how to prevent/mitigate]\n\n\
            Here are the diffs to analyze:\n\n"
        );

        for diff in diffs {
            prompt.push_str(&format!("### File: {}\n```diff\n{}\n```\n\n", diff.file_path, diff.content));
        }

        let body = json!({
            "model": "kimi-k2-0711-preview",
            "messages": [
                {
                    "role": "system",
                    "content": "You are an expert code reviewer specializing in identifying breaking changes and potential issues in code modifications."
                },
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "temperature": 0.3
        });

        let response = client
            .post("https://api.moonshot.cn/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to send request to Kimi API")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            anyhow::bail!("API request failed with status {}: {}", status, error_text);
        }

        let kimi_response: KimiResponse = response.json().await
            .context("Failed to parse API response")?;

        if kimi_response.choices.is_empty() {
            anyhow::bail!("No response from Kimi API");
        }

        Ok(kimi_response.choices[0].message.content.clone())
    }

    async fn review_changes(&self) -> Result<()> {
        println!("🔍 Validating git repository...");
        self.validate_git_repository()?;

        println!("📊 Getting unstaged changes...");
        let diffs = self.get_unstaged_changes()?;

        if diffs.is_empty() {
            println!("✅ No unstaged changes found in the repository.");
            return Ok(());
        }

        println!("📝 Found {} modified file(s):", diffs.len());
        for diff in &diffs {
            println!("  - {}", diff.file_path);
        }

        println!("\n🤖 Analyzing changes with AI...");
        let analysis = self.analyze_with_kimi(&diffs).await?;

        println!("🔍 CODE REVIEW ANALYSIS");
        println!("{}", "=".repeat(80));
        println!("{}", analysis);
        println!("{}", "=".repeat(80));

        Ok(())
    }
}

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
