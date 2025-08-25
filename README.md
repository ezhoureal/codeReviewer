# Overview
This is an AI agent that takes in a directory of a git repository and reviews the code changes to ensure forward compatibility of that repository.

# Strategy
1. initialization
2. collect git diff
3. prompt construction
4. Send to LLM
5. Output

# Usage
run `cargo run [directory]`, where [directory] points to the git repository you want to analyze.