#!/bin/sh
set -eu
root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' 0
mkdir -p "$work/bin" "$work/install"
printf '%s\n' '#!/bin/sh' 'echo fixture-v1' > "$work/artifact"
cat > "$work/bin/curl" <<'MOCK'
#!/bin/sh
set -eu
while [ "$#" -gt 0 ]; do
  case "$1" in --output) out=$2; shift 2;; *) url=$1; shift;; esac
done
case "$url" in
 *.sha256) if [ "${BAD_CHECKSUM:-0}" = 1 ]; then printf '%064d\n' 0 > "$out"; else sha256sum "$FIXTURE" > "$out"; fi ;;
 *) [ "${FAIL_DOWNLOAD:-0}" = 0 ] || exit 22; cp "$FIXTURE" "$out" ;;
esac
MOCK
chmod +x "$work/bin/curl"
export PATH="$work/bin:$PATH" CODEXY_INSTALL_DIR="$work/install" FIXTURE="$work/artifact"
sh "$root/scripts/install.sh" >/dev/null
[ "$("$work/install/codexy")" = fixture-v1 ]
printf '%s\n' '#!/bin/sh' 'echo fixture-v2' > "$work/artifact"
sh "$root/scripts/install.sh" >/dev/null
[ "$("$work/install/codexy")" = fixture-v2 ]
if BAD_CHECKSUM=1 sh "$root/scripts/install.sh" 2>/dev/null; then exit 1; fi
[ "$("$work/install/codexy")" = fixture-v2 ]
if FAIL_DOWNLOAD=1 sh "$root/scripts/install.sh" 2>/dev/null; then exit 1; fi
[ "$("$work/install/codexy")" = fixture-v2 ]
[ "$(find "$work/install" -type f | wc -l)" -eq 1 ]
printf '%s\n' 'Installer: install, update, checksum/download failure preservation passed.'
