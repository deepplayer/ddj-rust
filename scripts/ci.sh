#!/usr/bin/env bash
# 远端 CI：拉代码 → 格式 → lint → 测试 → release 构建
#
# 用法（在远端执行）：
#   bash /workspace/ddj-rust/scripts/ci.sh            # 全流程
#   SKIP_PULL=1 bash /workspace/ddj-rust/scripts/ci.sh  # 跳过 git pull（本地改动调试用）
#
# 产物：target/release/ddj，末尾打印大小与 sha256，方便用 web-shell 取回。

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

step() { printf '\n=== %s ===\n' "$1"; }

if [ "${SKIP_PULL:-0}" != "1" ]; then
  step "git pull"
  git pull --ff-only
fi

step "fmt"
RUSTFLAGS="-C link-arg=-fuse-ld=lld" cargo fmt --all -- --check

step "clippy"
RUSTFLAGS="-C link-arg=-fuse-ld=lld" cargo clippy --workspace --all-targets -- -D warnings

step "test"
RUSTFLAGS="-C link-arg=-fuse-ld=lld" cargo test --workspace

step "release build"
RUSTFLAGS="-C link-arg=-fuse-ld=lld" cargo build --release -p ddj-cli

step "artifact"
ls -la target/release/ddj
sha256sum target/release/ddj
