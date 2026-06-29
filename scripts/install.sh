#!/usr/bin/env sh
set -eu

repo="Lumisia/aho__ai-handoff"
version="latest"
yes=""
dry_run=""
agents=""

usage() {
  cat <<'EOF'
Usage:
  sh install.sh [--yes] [--dry-run] [--only codex|claude] [--version vX.Y.Z]

Installs the ai-handoff CLI from GitHub Releases, then runs:
  ai-handoff install
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --yes|-y)
      yes="--yes"
      ;;
    --dry-run)
      dry_run="--dry-run"
      ;;
    --only)
      shift
      [ "$#" -gt 0 ] || { echo "missing value for --only" >&2; exit 2; }
      case "$1" in
        codex|claude|claude-code)
          agents="--agents $1"
          ;;
        *)
          echo "unknown --only value: $1" >&2
          exit 2
          ;;
      esac
      ;;
    --version)
      shift
      [ "$#" -gt 0 ] || { echo "missing value for --version" >&2; exit 2; }
      version="$1"
      ;;
    --with-gui)
      echo "--with-gui is not available from this CLI installer yet." >&2
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
done

need() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "missing required command: $1" >&2
    exit 1
  }
}

need curl

os="$(uname -s)"
arch="$(uname -m)"

case "$os" in
  Darwin) os_name="darwin" ;;
  Linux) os_name="linux" ;;
  MINGW*|MSYS*|CYGWIN*) os_name="windows" ;;
  *) echo "unsupported OS: $os" >&2; exit 1 ;;
esac

case "$arch" in
  x86_64|amd64) arch_name="x86_64" ;;
  arm64|aarch64) arch_name="aarch64" ;;
  *) echo "unsupported architecture: $arch" >&2; exit 1 ;;
esac

if [ "$os_name" = "windows" ]; then
  ext="zip"
  exe_name="ai-handoff.exe"
  need unzip
else
  ext="tar.gz"
  exe_name="ai-handoff"
  need tar
fi

artifact="ai-handoff-cli-${os_name}-${arch_name}.${ext}"
if [ "$version" = "latest" ]; then
  url="https://github.com/${repo}/releases/latest/download/${artifact}"
else
  url="https://github.com/${repo}/releases/download/${version}/${artifact}"
fi

home="${AI_HANDOFF_HOME:-$HOME/.ai-handoff}"
bin_dir="$home/bin"
tmp_dir="${TMPDIR:-/tmp}/ai-handoff-install-$$"
archive="$tmp_dir/$artifact"

mkdir -p "$tmp_dir" "$bin_dir"
cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT INT TERM

echo "Downloading $url"
curl -fsSL "$url" -o "$archive"

if [ "$ext" = "zip" ]; then
  unzip -q "$archive" -d "$tmp_dir"
else
  tar -xzf "$archive" -C "$tmp_dir"
fi

found="$(find "$tmp_dir" -type f -name "$exe_name" | head -n 1)"
[ -n "$found" ] || {
  echo "artifact did not contain $exe_name" >&2
  exit 1
}

dest="$bin_dir/$exe_name"
cp "$found" "$dest"
chmod +x "$dest" 2>/dev/null || true

echo "Installed $dest"
echo "Running ai-handoff install"

# shellcheck disable=SC2086
"$dest" install $dry_run $yes $agents

cat <<EOF

Done.
If your shell cannot find ai-handoff, add this directory to PATH:
  $bin_dir
EOF
