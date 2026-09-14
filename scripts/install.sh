#!/bin/sh

set -eu

repo='smturtle2/codexy'
install_dir=${CODEXY_INSTALL_DIR:-"$HOME/.local/bin"}

case "$(uname -s)" in
    Linux) os=linux ;;
    Darwin) os=darwin ;;
    *)
        printf '%s\n' 'error: supported systems are Linux and macOS' >&2
        exit 1
        ;;
esac

case "$(uname -m)" in
    x86_64|amd64) arch=x86_64 ;;
    aarch64|arm64) arch=aarch64 ;;
    *)
        printf '%s\n' 'error: supported architectures are x86_64 and aarch64' >&2
        exit 1
        ;;
esac

if [ "$os" = darwin ]; then
    target="${arch}-apple-darwin"
else
    target="${arch}-unknown-linux-gnu"
fi
base_url="https://github.com/${repo}/releases/latest/download"
binary_name="codexy-${target}"
checksum_name="${binary_name}.sha256"

mkdir -p "$install_dir"
if ! temp_binary=$(mktemp "${install_dir}/.codexy.XXXXXX"); then
    printf '%s\n' "error: cannot create temporary files in ${install_dir}" >&2
    exit 1
fi
if ! temp_checksum=$(mktemp "${install_dir}/.codexy-sha256.XXXXXX"); then
    rm -f "$temp_binary"
    printf '%s\n' "error: cannot create temporary files in ${install_dir}" >&2
    exit 1
fi

cleanup() {
    rm -f "$temp_binary" "$temp_checksum"
}
trap cleanup 0
trap 'exit 1' HUP INT TERM

download() {
    url=$1
    destination=$2
    if command -v curl >/dev/null 2>&1; then
        curl --fail --silent --show-error --location --proto '=https' --proto-redir '=https' --retry 3 --output "$destination" "$url"
    elif command -v wget >/dev/null 2>&1; then
        wget --https-only --quiet --output-document="$destination" "$url"
    else
        printf '%s\n' 'error: curl or wget is required' >&2
        return 1
    fi
}

download "${base_url}/${binary_name}" "$temp_binary" || {
    printf '%s\n' "error: failed to download ${binary_name}" >&2
    exit 1
}
download "${base_url}/${checksum_name}" "$temp_checksum" || {
    printf '%s\n' "error: failed to download ${checksum_name}" >&2
    exit 1
}

expected=$(awk 'NF { print $1; exit }' "$temp_checksum")
case "$expected" in
    ''|*[!0123456789abcdefABCDEF]*)
        printf '%s\n' 'error: invalid checksum file' >&2
        exit 1
        ;;
esac
[ "${#expected}" -eq 64 ] || {
    printf '%s\n' 'error: invalid SHA-256 checksum' >&2
    exit 1
}

if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$temp_binary" | awk '{ print $1 }')
elif command -v shasum >/dev/null 2>&1; then
    actual=$(shasum -a 256 "$temp_binary" | awk '{ print $1 }')
else
    printf '%s\n' 'error: sha256sum or shasum is required' >&2
    exit 1
fi
[ "$expected" = "$actual" ] || {
    printf '%s\n' 'error: checksum verification failed' >&2
    exit 1
}

chmod 0755 "$temp_binary"
mv -f "$temp_binary" "${install_dir}/codexy"
cleanup
trap - 0 HUP INT TERM
printf '%s\n' "Installed codexy to ${install_dir}/codexy"
