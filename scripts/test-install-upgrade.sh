#!/usr/bin/env sh
# Exercise the real installer with local fixtures, without network or user data.
set -eu

if [ "$(uname -s)" != Linux ]; then
  echo "install upgrade regression skipped: requires the Linux executable lock"
  exit 0
fi

script_dir="$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)"
installer="$script_dir/install.sh"
fixture_root="$(mktemp -d "${TMPDIR:-/tmp}/ai-handoff-upgrade-test-XXXXXX")"
fixture="$fixture_root/space in paths"
cli_pid=""
host_pid=""
cleanup() {
  if [ -n "$cli_pid" ]; then kill "$cli_pid" 2>/dev/null || true; wait "$cli_pid" 2>/dev/null || true; fi
  if [ -n "$host_pid" ]; then kill "$host_pid" 2>/dev/null || true; wait "$host_pid" 2>/dev/null || true; fi
  rm -rf "$fixture_root"
}
trap cleanup EXIT INT TERM
mkdir -p "$fixture/tools" "$fixture/release" "$fixture/tmp"

cat > "$fixture/probe.c" <<'EOF'
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

int main(int argc, char **argv) {
    if (argc > 2 && strcmp(argv[1], "--hold") == 0) {
        FILE *ready = fopen(argv[2], "w");
        if (ready == NULL) return 3;
        fclose(ready);
        for (;;) pause();
    }
    if (argc > 1 && strcmp(argv[1], "install") == 0) {
        const char *path = getenv("INSTALL_PROBE");
        if (path == NULL) return 4;
        FILE *probe = fopen(path, "w");
        if (probe == NULL) return 5;
        fprintf(probe, "%d\n", BUILD_ID);
        fclose(probe);
    }
    printf("%d\n", BUILD_ID);
    return 0;
}
EOF
cc -DBUILD_ID=1 "$fixture/probe.c" -o "$fixture/old-binary"
cc -DBUILD_ID=2 "$fixture/probe.c" -o "$fixture/release/ai-handoff"
cp "$fixture/release/ai-handoff" "$fixture/release/ai-handoff-host"
case "$(uname -m)" in
  x86_64|amd64) arch=x86_64 ;;
  arm64|aarch64) arch=aarch64 ;;
  *) echo "unsupported test architecture" >&2; exit 1 ;;
esac
artifact="ai-handoff-cli-linux-$arch.tar.gz"
tar -czf "$fixture/$artifact" -C "$fixture/release" ai-handoff ai-handoff-host
(cd "$fixture" && sha256sum "$artifact" > "$artifact.sha256")
REAL_CP="$(command -v cp)"
FIXTURE_ARCHIVE="$fixture/$artifact"
FIXTURE_CHECKSUM="$fixture/$artifact.sha256"
export REAL_CP FIXTURE_ARCHIVE FIXTURE_CHECKSUM

cat > "$fixture/tools/curl" <<'EOF'
#!/usr/bin/env sh
set -eu
url=""
output=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o) shift; output="$1" ;;
    -*) ;;
    *) url="$1" ;;
  esac
  shift
done
case "$url" in
  https://github.com/*/releases/download/v0.0.0/*.sha256)
    exec "$REAL_CP" "$FIXTURE_CHECKSUM" "$output" ;;
  https://github.com/*/releases/download/v0.0.0/*.tar.gz)
    exec "$REAL_CP" "$FIXTURE_ARCHIVE" "$output" ;;
  *) echo "unexpected fixture URL: $url" >&2; exit 9 ;;
esac
EOF
cat > "$fixture/tools/cp" <<'EOF'
#!/usr/bin/env sh
set -eu
if [ "${FAIL_HOST_COPY:-}" = 1 ] && [ "${1##*/}" = ai-handoff-host ]; then
  echo "injected host staging failure" >&2
  exit 7
fi
exec "$REAL_CP" "$@"
EOF
chmod +x "$fixture/tools/curl" "$fixture/tools/cp"

prepare_case() {
  case_root="$fixture/$1"
  bin_dir="$case_root/ai-home/bin"
  mkdir -p "$bin_dir" "$case_root/home"
  "$REAL_CP" "$fixture/old-binary" "$bin_dir/ai-handoff"
  "$REAL_CP" "$fixture/old-binary" "$bin_dir/ai-handoff-host"
}
run_installer() {
  HOME="$case_root/home" AI_HANDOFF_HOME="$case_root/ai-home" \
    TMPDIR="$fixture/tmp" INSTALL_PROBE="$case_root/install-ran" \
    PATH="$fixture/tools:$PATH" sh "$installer" --yes --version v0.0.0
}
assert_no_staging() {
  [ -z "$(find "$bin_dir" -name '.ai-handoff-install-*' -print)" ] || {
    echo "installer left its staging directory" >&2; exit 1;
  }
}

prepare_case running
"$bin_dir/ai-handoff" --hold "$case_root/cli-ready" &
cli_pid=$!
"$bin_dir/ai-handoff-host" --hold "$case_root/host-ready" &
host_pid=$!
attempt=0
while [ ! -f "$case_root/cli-ready" ] || [ ! -f "$case_root/host-ready" ]; do
  attempt=$((attempt + 1))
  [ "$attempt" -lt 100 ] || { echo "native fixtures did not start" >&2; exit 1; }
  sleep 0.05
done
if ! run_installer > "$case_root/install.log" 2>&1; then
  cat "$case_root/install.log" >&2
  echo "upgrade failed while the old executables were running" >&2
  exit 1
fi
[ "$("$bin_dir/ai-handoff" --version)" = 2 ]
[ "$("$bin_dir/ai-handoff-host" --version)" = 2 ]
[ "$(cat "$case_root/install-ran")" = 2 ]
kill -0 "$cli_pid"
kill -0 "$host_pid"
assert_no_staging
echo "running binary upgrade verified"

prepare_case staging-failure
FAIL_HOST_COPY=1
export FAIL_HOST_COPY
if run_installer > "$case_root/install.log" 2>&1; then
  echo "installer accepted an injected host staging failure" >&2
  exit 1
fi
[ "$("$bin_dir/ai-handoff" --version)" = 1 ]
[ "$("$bin_dir/ai-handoff-host" --version)" = 1 ]
[ ! -e "$case_root/install-ran" ]
assert_no_staging
echo "failed staging preserved both installed binaries"
