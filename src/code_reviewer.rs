use anyhow::{Context, Result};
use reqwest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::env;
use std::process::Command;

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

#[derive(Debug, Clone)]
pub struct GitDiff {
    file_path: String,
    content: String,
}

#[derive(Debug)]
pub struct CodeReviewer {
    api_key: String,
    repo_path: String,
}

const API_KEY: &str = "MOONSHOT_API_KEY";

impl CodeReviewer {
    pub fn new(repo_path: String) -> Result<Self> {
        let api_key = env::var(API_KEY).context("MOONSHOT_API_KEY environment variable not set")?;

        Ok(CodeReviewer { api_key, repo_path })
    }

    fn new_with_api_key(repo_path: String, api_key: String) -> Self {
        CodeReviewer { api_key, repo_path }
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
            anyhow::bail!(
                "Failed to get git diff: {}",
                String::from_utf8_lossy(&output.stderr)
            );
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
            prompt.push_str(&format!(
                "### File: {}\n```diff\n{}\n```\n\n",
                diff.file_path, diff.content
            ));
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
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            anyhow::bail!("API request failed with status {}: {}", status, error_text);
        }

        let kimi_response: KimiResponse = response
            .json()
            .await
            .context("Failed to parse API response")?;

        if kimi_response.choices.is_empty() {
            anyhow::bail!("No response from Kimi API");
        }

        Ok(kimi_response.choices[0].message.content.clone())
    }

    pub async fn review_changes(&self) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use std::process::Command;
    use tempfile::TempDir;

    fn setup_git_repo() -> TempDir {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let repo_path = temp_dir.path();

        // Initialize git repository
        Command::new("git")
            .args(&["init"])
            .current_dir(repo_path)
            .output()
            .expect("Failed to initialize git repository");

        // Configure git user for tests
        Command::new("git")
            .args(&["config", "user.email", "test@example.com"])
            .current_dir(repo_path)
            .output()
            .expect("Failed to configure git user email");

        Command::new("git")
            .args(&["config", "user.name", "Test User"])
            .current_dir(repo_path)
            .output()
            .expect("Failed to configure git user name");

        temp_dir
    }

    fn create_test_file(repo_path: &std::path::Path, filename: &str, content: &str) {
        let file_path = repo_path.join(filename);
        let mut file = File::create(file_path).expect("Failed to create test file");
        file.write_all(content.as_bytes())
            .expect("Failed to write to test file");
    }

    fn git_add_and_commit(repo_path: &std::path::Path, message: &str) {
        Command::new("git")
            .args(&["add", "."])
            .current_dir(repo_path)
            .output()
            .expect("Failed to git add");

        Command::new("git")
            .args(&["commit", "-m", message])
            .current_dir(repo_path)
            .output()
            .expect("Failed to git commit");
    }

    #[test]
    fn test_new() {
        env::set_var(API_KEY, "test_key");
        let repo_path = "/tmp/test".to_string();

        let result = CodeReviewer::new(repo_path.clone());
        assert!(result.is_ok());

        let reviewer = result.unwrap();
        assert_eq!(reviewer.repo_path, repo_path);
        assert_eq!(reviewer.api_key, "test_key");
        env::remove_var(API_KEY);
        let repo_path = "/tmp/test".to_string();

        let result = CodeReviewer::new(repo_path);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("MOONSHOT_API_KEY environment variable not set"));
    }

    #[test]
    fn test_validate_git_repository_valid() {
        let temp_dir = setup_git_repo();
        let reviewer = CodeReviewer::new_with_api_key(
            temp_dir.path().to_string_lossy().to_string(),
            "test_key".to_string(),
        );

        let result = reviewer.validate_git_repository();
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_git_repository_invalid() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let reviewer = CodeReviewer::new_with_api_key(
            temp_dir.path().to_string_lossy().to_string(),
            "test_key".to_string(),
        );

        let result = reviewer.validate_git_repository();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("not a git repository"));
    }

    #[test]
    fn test_get_unstaged_changes_no_changes() {
        let temp_dir = setup_git_repo();
        let repo_path = temp_dir.path();

        // Create and commit a file
        create_test_file(repo_path, "test.txt", "initial content");
        git_add_and_commit(repo_path, "Initial commit");

        let reviewer = CodeReviewer::new_with_api_key(
            repo_path.to_string_lossy().to_string(),
            "test_key".to_string(),
        );

        let result = reviewer.get_unstaged_changes();
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_get_unstaged_changes_with_modifications() {
        let temp_dir = setup_git_repo();
        let repo_path = temp_dir.path();

        // Create and commit a file
        create_test_file(repo_path, "test.txt", "initial content");
        git_add_and_commit(repo_path, "Initial commit");

        // Modify the file
        create_test_file(repo_path, "test.txt", "modified content");

        let reviewer = CodeReviewer::new_with_api_key(
            repo_path.to_string_lossy().to_string(),
            "test_key".to_string(),
        );

        let result = reviewer.get_unstaged_changes();
        assert!(result.is_ok());

        let diffs = result.unwrap();
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].file_path, "test.txt");
        assert!(diffs[0].content.contains("modified content"));
        assert!(diffs[0].content.contains("initial content"));
    }
}
