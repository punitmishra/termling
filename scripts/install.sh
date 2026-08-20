#!/bin/sh
# termling installer — download a prebuilt binary and drop it in your PATH.
#
#   curl -fsSL https://raw.githubusercontent.com/punitmishra/termling/main/scripts/install.sh | sh
#
# No sudo: installs to ${TERMLING_INSTALL_DIR:-$HOME/.local/bin}. This is the
# ONLY part of termling that touches the network — the binary itself never does.
# Overrides: TERMLING_VERSION (e.g. v0.1.0), TERMLING_INSTALL_DIR.
set -eu

REPO="punitmishra/termling"
BIN="termling"
INSTALL_DIR="${TERMLING_INSTALL_DIR:-$HOME/.local/bin}"

err() {
	printf 'termling install: %s\n' "$1" >&2
	exit 1
}

need() {
	command -v "$1" >/dev/null 2>&1 || err "required tool '$1' not found"
}

need curl
need tar

# --- target triple from OS + arch ------------------------------------------
os="$(uname -s)"
arch="$(uname -m)"
case "$os" in
Darwin)
	case "$arch" in
	arm64 | aarch64) target="aarch64-apple-darwin" ;;
	x86_64) target="x86_64-apple-darwin" ;;
	*) err "unsupported macOS arch: $arch" ;;
	esac
	;;
Linux)
	case "$arch" in
	x86_64) target="x86_64-unknown-linux-gnu" ;;
	aarch64 | arm64) err "no prebuilt Linux arm64 build yet — install with cargo: cargo install $BIN" ;;
	*) err "unsupported Linux arch: $arch" ;;
	esac
	;;
*)
	err "unsupported OS: $os (try: cargo install $BIN)"
	;;
esac

# --- resolve the version ----------------------------------------------------
# The redirect trick: /releases/latest 302s to /releases/tag/vX.Y.Z. Reading the
# effective URL avoids the GitHub API (no rate limit, no jq). TERMLING_VERSION
# overrides it.
if [ -n "${TERMLING_VERSION:-}" ]; then
	version="$TERMLING_VERSION"
else
	latest_url="$(curl -fsSLI -o /dev/null -w '%{url_effective}' \
		"https://github.com/$REPO/releases/latest")" ||
		err "could not reach GitHub to resolve the latest version"
	version="${latest_url##*/}"
fi
# The git tag carries a leading 'v' (v0.1.0); release *assets* drop it
# (termling-0.1.0-<target>.tar.gz). Normalize both so either form of
# TERMLING_VERSION works and the URL path (tag) and filename (bare) each get
# the right shape.
ver="${version#v}"
case "$ver" in
[0-9]*) ;;
*) err "could not determine a release version (got '$version')" ;;
esac
tag="v$ver"

# --- download + verify ------------------------------------------------------
archive="$BIN-$ver-$target.tar.gz"
base="https://github.com/$REPO/releases/download/$tag"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

printf 'termling: downloading %s (%s)…\n' "$tag" "$target"
curl -fsSL "$base/$archive" -o "$tmp/$archive" ||
	err "download failed: $base/$archive"
curl -fsSL "$base/$archive.sha256" -o "$tmp/$archive.sha256" ||
	err "checksum download failed: $base/$archive.sha256"

# The .sha256 asset holds a bare hash; build a "<hash>  <file>" line to -c.
expected="$(cut -d' ' -f1 "$tmp/$archive.sha256")"
(
	cd "$tmp"
	line="$expected  $archive"
	if command -v shasum >/dev/null 2>&1; then
		printf '%s\n' "$line" | shasum -a 256 -c - >/dev/null 2>&1 ||
			err "checksum mismatch — refusing to install"
	elif command -v sha256sum >/dev/null 2>&1; then
		printf '%s\n' "$line" | sha256sum -c - >/dev/null 2>&1 ||
			err "checksum mismatch — refusing to install"
	else
		printf 'termling: no shasum/sha256sum found — skipping checksum verify\n' >&2
	fi
)

tar -xzf "$tmp/$archive" -C "$tmp" ||
	err "could not extract $archive"
[ -f "$tmp/$BIN" ] || err "archive did not contain '$BIN'"

# --- install ----------------------------------------------------------------
mkdir -p "$INSTALL_DIR"
install -m 0755 "$tmp/$BIN" "$INSTALL_DIR/$BIN" 2>/dev/null ||
	{ cp "$tmp/$BIN" "$INSTALL_DIR/$BIN" && chmod 0755 "$INSTALL_DIR/$BIN"; } ||
	err "could not install to $INSTALL_DIR"

printf 'termling: installed to %s/%s\n' "$INSTALL_DIR" "$BIN"

# --- PATH + hook hints ------------------------------------------------------
case ":$PATH:" in
*":$INSTALL_DIR:"*) ;;
*)
	printf '\n%s is not on your PATH. Add it:\n' "$INSTALL_DIR"
	printf '  export PATH="%s:$PATH"\n' "$INSTALL_DIR"
	;;
esac

cat <<EOF

Next: add the hook to your shell rc so the companion reacts (and auto-spawns):
  zsh :  eval "\$($BIN init zsh)"       # ~/.zshrc
  bash:  eval "\$($BIN init bash)"      # ~/.bashrc
  fish:  $BIN init fish | source        # ~/.config/fish/config.fish

Then open a new window. Run '$BIN doctor' if it doesn't appear.
EOF
