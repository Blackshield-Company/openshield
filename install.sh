#!/usr/bin/env bash
set -euo pipefail

# OpenShark Installer
# Usage: curl -sSL https://raw.githubusercontent.com/synthalorian/openshark/main/install.sh | bash

REPO="https://github.com/synthalorian/openshark.git"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/share/openshark}"
BIN_DIR="${BIN_DIR:-$HOME/.local/bin}"

echo "🦞 OpenShark Installer"
echo "======================"

# Check for Rust
check_rust() {
    if ! command -v rustc &> /dev/null; then
        echo "❌ Rust not found. Installing via rustup..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source "$HOME/.cargo/env"
    fi
    echo "✅ Rust $(rustc --version)"
}

# Clone or update repo
setup_repo() {
    if [ -d "$INSTALL_DIR/.git" ]; then
        echo "🔄 Updating existing OpenShark..."
        cd "$INSTALL_DIR"
        git pull
    else
        echo "📥 Cloning OpenShark..."
        mkdir -p "$(dirname "$INSTALL_DIR")"
        git clone "$REPO" "$INSTALL_DIR"
        cd "$INSTALL_DIR"
    fi
}

# Build release binary
build() {
    echo "🔨 Building OpenShark (release)..."
    cargo build --release
}

# Install binary
install_binary() {
    mkdir -p "$BIN_DIR"
    cp "$INSTALL_DIR/target/release/openshark" "$BIN_DIR/"
    chmod +x "$BIN_DIR/openshark"
    echo "✅ Installed openshark to $BIN_DIR"
}

# Setup shell integration
setup_shell() {
    local shell_rc=""
    case "${SHELL##*/}" in
        bash) shell_rc="$HOME/.bashrc" ;;
        zsh)  shell_rc="$HOME/.zshrc" ;;
        fish) shell_rc="$HOME/.config/fish/config.fish" ;;
        *)    shell_rc="" ;;
    esac

    if [ -n "$shell_rc" ] && [ -f "$shell_rc" ]; then
        if ! grep -q "$BIN_DIR" "$shell_rc" 2>/dev/null; then
            echo "export PATH=\"$BIN_DIR:\$PATH\"" >> "$shell_rc"
            echo "✅ Added $BIN_DIR to PATH in $shell_rc"
            echo "   Run: source $shell_rc"
        fi
    fi

    # Also check if PATH already includes it
    if [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
        export PATH="$BIN_DIR:$PATH"
    fi
}

# Create config directory
setup_config() {
    local config_dir="$HOME/.config/openshark"
    mkdir -p "$config_dir"
    echo "✅ Config directory: $config_dir"
    echo "   Set your provider API key environment variable before running"
    echo "   Examples: OPENAI_API_KEY, ANTHROPIC_API_KEY, KIMI_API_KEY, XAI_API_KEY"
}

# Main
main() {
    check_rust
    setup_repo
    build
    install_binary
    setup_shell
    setup_config

    echo ""
    echo "🎉 OpenShark installed successfully!"
    echo ""
    echo "Usage:"
    echo "  openshark           Launch TUI (default)"
    echo "  openshark tui       Launch TUI explicitly"
    echo "  openshark chat      One-shot chat"
    echo "  openshark agent     Run agent on a task"
    echo "  openshark models    List available models"
    echo "  openshark stats     View usage statistics"
    echo "  openshark config    Show configuration"
    echo "  openshark setup     Interactive setup"
    echo ""
    echo "Environment variables:"
    echo "  OPENAI_API_KEY      Your OpenAI API key"
    echo "  ANTHROPIC_API_KEY   Your Anthropic API key"
    echo "  KIMI_API_KEY        Your Kimi API key"
    echo "  XAI_API_KEY         Your xAI API key"
    echo "  SOUL_NAME           Set to 'blank' for blank slate, or 'synthclaw' (default)"
    echo ""
    echo "Run 'openshark' to start!"
}

main "$@"
