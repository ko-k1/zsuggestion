//! Authored command metadata used before (and instead of) help probing.
//!
//! `SHELL_BUILTINS` is always part of the catalog. Authored descriptions win
//! over parsed man/help text so common tools keep stable, reviewed wording.

use std::path::Path;

pub(crate) fn description(name: &str) -> Option<&'static str> {
    Some(match name {
        "ansible" => "Define and run automation tasks",
        "appwrite" => "Manage Appwrite projects and services",
        "arch" => "Print architecture type or run a universal binary",
        "asr" => "Apple Software Restore; copy volumes and disk images",
        "atlas" => "CLI tool to manage MongoDB Atlas",
        "aws" => "Official command line interface for Amazon Web Services",
        "aws-vault" => "Securely store and access AWS credentials",
        "bash" => "GNU Bourne Again shell",
        "brew" => "The missing package manager for macOS",
        "cargo" => "Rust package manager and build tool",
        "cmake" => "Configure, build, and test software projects",
        "code" => "Open Visual Studio Code",
        "curl" => "Transfer data from or to a server",
        "docker" => "Build and run applications in containers",
        "fd" => "Fast and user-friendly file finder",
        "fzf" => "Command-line fuzzy finder",
        "gh" => "GitHub command line interface",
        "git" => "Distributed version control system",
        "go" => "Build and manage Go source code",
        "iris" => "Interactive shell assistant",
        "jq" => "Process and transform JSON",
        "kubectl" => "Control Kubernetes clusters",
        "make" => "Maintain and build groups of programs",
        "node" => "Run JavaScript with Node.js",
        "npm" => "JavaScript package manager",
        "nvim" => "Edit text with Neovim",
        "pnpm" => "Fast, disk-efficient JavaScript package manager",
        "python" | "python3" => "Run the Python interpreter",
        "rg" => "Recursively search files with ripgrep",
        "rustc" => "Compile Rust source code",
        "ssh" => "OpenSSH remote login client",
        "tmux" => "Terminal multiplexer",
        "yarn" => "JavaScript package manager",
        "zsh" => "Z shell command interpreter",
        _ => return None,
    })
}

pub(crate) fn fallback(path: &Path) -> String {
    let path = path.to_string_lossy();
    if path.contains("/.cargo/bin/") {
        "Executable installed by Cargo".to_owned()
    } else if path.contains("/homebrew/") || path.contains("/Cellar/") {
        "Homebrew command".to_owned()
    } else if path.contains("/.local/bin/") || path.contains("/bin/") && path.contains("/Users/") {
        "User-installed command".to_owned()
    } else {
        "System command".to_owned()
    }
}

pub(crate) const SHELL_BUILTINS: &[(&str, &str)] = &[
    ("alias", "Define or display shell aliases"),
    ("autoload", "Mark shell functions for automatic loading"),
    ("bg", "Resume jobs in the background"),
    ("cd", "Change the current working directory"),
    ("command", "Execute a command without shell function lookup"),
    ("export", "Set environment variables for child processes"),
    ("fg", "Bring jobs into the foreground"),
    ("jobs", "Display active shell jobs"),
    ("setopt", "Enable Zsh options"),
    (
        "source",
        "Execute commands from a file in the current shell",
    ),
    ("typeset", "Declare shell variables and attributes"),
    ("unalias", "Remove shell alias definitions"),
    ("unset", "Remove shell variables or functions"),
    ("unsetopt", "Disable Zsh options"),
];
