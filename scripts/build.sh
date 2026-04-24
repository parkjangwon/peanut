#!/bin/bash
set -e

PROJECT_ROOT=$(pwd)
CONSOLE_DIR="$PROJECT_ROOT/peanut-console"

echo "🥜 Building Peanut Project..."

echo "Step 1: Building Console (Next.js)..."
cd "$CONSOLE_DIR"
npm install
npx next build --webpack

echo "Step 2: Building Backend (Rust)..."
cd "$PROJECT_ROOT"
cargo build --release

echo "✨ Done! Single binary available at: target/release/peanut"
echo "You can run it with: ./target/release/peanut"
