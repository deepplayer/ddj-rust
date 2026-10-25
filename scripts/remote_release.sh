#!/usr/bin/env bash
# 远端 release 构建（幂等、可重入）
#
# 为什么这么写：
# web-shell 每次请求可能落到不同的容器节点，但 /workspace 是共享卷。
# 所以构建状态只能存在 /workspace 里，且必须能判断「上次是在别的节点上
# 编了一半」这种情况（不同节点的 target 缓存会互相污染，导致
# `Exec format error`）。
#
# 用法：bash /workspace/ddj-rust/scripts/remote_release.sh
# 产物：/workspace/ddj-rust/target/release/ddj + build_result.json

set -uo pipefail

REPO=/workspace/ddj-rust
STATE=$REPO/build_result.json
LOCK=$REPO/.build.lock
NODE=$(hostname)

cd "$REPO" || { echo "缺少 $REPO"; exit 1; }

# 单实例：避免两个节点同时编译同一份 target
exec 9>"$LOCK"
if ! flock -w 1800 9; then
  echo "LOCK_TIMEOUT"
  exit 2
fi

COMMIT=$(git rev-parse HEAD)
SHORT=$(git rev-parse --short HEAD)

echo "node=$NODE commit=$SHORT"

# 校验工作区与 gitee/main 一致（防止编到旧代码）
git fetch gitee --prune -q && git reset --hard gitee/main -q && git clean -fdq -e target -e build_out.txt -e build_result.json -e .build_node -e .build.lock
COMMIT=$(git rev-parse HEAD)
SHORT=$(git rev-parse --short HEAD)
echo "synced commit=$SHORT"

# 跨节点缓存污染检测：记录上一次编译的节点，不同则清掉 target
PREV_NODE=""
[ -f "$REPO/.build_node" ] && PREV_NODE=$(cat "$REPO/.build_node")
if [ -n "$PREV_NODE" ] && [ "$PREV_NODE" != "$NODE" ]; then
  echo "检测到节点切换（$PREV_NODE → $NODE），清理 target 避免缓存污染"
  rm -rf "$REPO/target"
fi
echo "$NODE" > "$REPO/.build_node"

export RUSTFLAGS="-C link-arg=-fuse-ld=lld"
export CARGO_INCREMENTAL=0

echo "=== clippy ==="
if ! cargo clippy --workspace --exclude ddj-pb --lib --bins -- -D warnings 2>&1 | tail -20; then
  echo "CLIPPY_FAILED"; exit 3
fi
[ "${PIPESTATUS[0]}" -ne 0 ] && { echo "CLIPPY_FAILED"; exit 3; }

echo "=== test ==="
cargo test --workspace --no-fail-fast 2>&1 | grep -E "^(test result|error|failures:)" | tail -40
TEST_RC=${PIPESTATUS[0]}

echo "=== release ==="
cargo build --release -p ddj-cli 2>&1 | tail -5
BUILD_RC=$?

SHA=""
SIZE=""
if [ -x target/release/ddj ]; then
  SHA=$(sha256sum target/release/ddj | cut -d' ' -f1)
  SIZE=$(stat -c '%s' target/release/ddj)
fi

cat > "$STATE" <<JSON
{"node":"$NODE","commit":"$COMMIT","short":"$SHORT","test_rc":$TEST_RC,"build_rc":$BUILD_RC,"sha256":"$SHA","size":${SIZE:-0},"finished_at":"$(date -Iseconds)"}
JSON

echo "BUILD_RESULT: $(cat "$STATE")"
if [ "$TEST_RC" -ne 0 ] || [ "$BUILD_RC" -ne 0 ] || [ -z "$SHA" ]; then
  echo "BUILD_FAILED"; exit 4
fi
echo "BUILD_OK"
