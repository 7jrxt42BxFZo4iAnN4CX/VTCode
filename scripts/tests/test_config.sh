#!/usr/bin/env bash

# Test script for VT Code config loading from the canonical user directory

echo "Testing VT Code configuration loading..."

CONFIG_DIR=""
case "${VTCODE_CONFIG:-}" in
    /*) CONFIG_DIR="$VTCODE_CONFIG" ;;
esac
if [ -z "$CONFIG_DIR" ]; then
    case "${XDG_CONFIG_HOME:-}" in
        /*) CONFIG_DIR="$XDG_CONFIG_HOME/vtcode" ;;
        *) CONFIG_DIR="$HOME/.config/vtcode" ;;
    esac
fi

# Check if the canonical config directory exists
if [ -d "$CONFIG_DIR" ]; then
    echo "✓ Found $CONFIG_DIR"

    if [ -f "$CONFIG_DIR/vtcode.toml" ]; then
        echo "✓ Found $CONFIG_DIR/vtcode.toml"

        # Check if the file has content
        if [ -s "$CONFIG_DIR/vtcode.toml" ]; then
            echo "✓ Configuration file has content"
        else
            echo "⚠ Configuration file is empty"
        fi
    else
        echo "✗ $CONFIG_DIR/vtcode.toml not found"
    fi
else
    echo "✗ $CONFIG_DIR not found"
fi

echo "Test completed."
