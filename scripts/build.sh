#!/bin/bash
set -e

PROJECT_ROOT=$(pwd)

echo "🥜 Building Peanut Project..."
echo "Step 1: Building Backend (Rust)..."
cd "$PROJECT_ROOT"
cargo build --release

echo "✨ Done! Backend binary available at: target/release/peanut"
echo "Peanut is currently packaged in API-first mode."
echo "You can run it with: ./target/release/peanut"
