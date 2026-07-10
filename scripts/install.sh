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

The default "latest" version resolves to the highest stable vX.Y.Z release
tag, not GitHub's mutable "Latest" badge.
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
  host_name="ai-handoff-host.exe"
  need unzip
else
  ext="tar.gz"
  exe_name="ai-handoff"
  host_name="ai-handoff-host"
  need tar
fi

artifact="ai-handoff-cli-${os_name}-${arch_name}.${ext}"
resolve_latest_release_tag() {
  api="https://api.github.com/repos/${repo}/releases?per_page=100"
  json="$(curl -fsSL \
    -H "Accept: application/vnd.github+json" \
    -H "User-Agent: ai-handoff-installer" \
    "$api")" || {
      echo "could not resolve latest release from $api" >&2
      exit 1
    }

  tags="$(printf '%s\n' "$json" | awk '
    /"tag_name"[[:space:]]*:/ {
      tag = $0
      sub(/^.*"tag_name"[[:space:]]*:[[:space:]]*"/, "", tag)
      sub(/".*$/, "", tag)
    }
    /"draft"[[:space:]]*:/ {
      draft = ($0 ~ /true/)
    }
    /"prerelease"[[:space:]]*:/ {
      prerelease = ($0 ~ /true/)
      if (!draft && !prerelease && tag ~ /^v?[0-9]+\.[0-9]+\.[0-9]+$/) {
        print tag
      }
      tag = ""
      draft = 0
      prerelease = 0
    }
  ')"
  latest="$(
    printf '%s\n' "$tags" |
      while IFS= read -r tag; do
        [ -n "$tag" ] || continue
        clean=${tag#v}
        old_ifs=$IFS
        IFS=.
        set -- $clean
        IFS=$old_ifs
        printf '%09d.%09d.%09d %s\n' "$1" "$2" "$3" "$tag"
      done |
      sort -r |
      sed -n '1s/^[^ ]* //p'
  )"

  [ -n "$latest" ] || {
    echo "could not find a stable vX.Y.Z release for ${repo}" >&2
    exit 1
  }
  printf '%s\n' "$latest"
}

release_version="$version"
if [ "$version" = "latest" ]; then
  release_version="$(resolve_latest_release_tag)"
  echo "Resolved latest release: $release_version"
fi
url="https://github.com/${repo}/releases/download/${release_version}/${artifact}"

home="${AI_HANDOFF_HOME:-$HOME/.ai-handoff}"
bin_dir="$home/bin"
tmp_dir="${TMPDIR:-/tmp}/ai-handoff-install-$$"
archive="$tmp_dir/$artifact"
checksum="$archive.sha256"

mkdir -p "$tmp_dir" "$bin_dir"
cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT INT TERM

echo "Downloading $url"
curl -fsSL "$url" -o "$archive"
echo "Downloading $url.sha256"
curl -fsSL "$url.sha256" -o "$checksum"

verify_checksum() {
  if command -v sha256sum >/dev/null 2>&1; then
    (cd "$tmp_dir" && sha256sum -c "$artifact.sha256")
  elif command -v shasum >/dev/null 2>&1; then
    (cd "$tmp_dir" && shasum -a 256 -c "$artifact.sha256")
  else
    echo "missing required command: sha256sum or shasum" >&2
    exit 1
  fi
}

verify_checksum

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
host_found="$(find "$tmp_dir" -type f -name "$host_name" | head -n 1)"
[ -n "$host_found" ] || {
  echo "artifact did not contain $host_name" >&2
  exit 1
}

dest="$bin_dir/$exe_name"
host_dest="$bin_dir/$host_name"
cp "$found" "$dest"
cp "$host_found" "$host_dest"
chmod +x "$dest" 2>/dev/null || true
chmod +x "$host_dest" 2>/dev/null || true

echo "Installed $dest"
echo "Installed $host_dest"
echo "Running ai-handoff install"

# shellcheck disable=SC2086
"$dest" install $dry_run $yes $agents

cat <<EOF

Done.
If your shell cannot find ai-handoff, add this directory to PATH:
  $bin_dir
EOF
