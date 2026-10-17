#!/usr/bin/env bash
# Build Ruche and wrap it in a local macOS app bundle: target/Ruche.app.
#
# Mirrors the "Create app bundle" step of .github/workflows/build.yml with the
# Ruche identity, and signs the bundle ad hoc so it runs locally. It is not
# notarised: Gatekeeper still needs a Developer ID for distribution.
#
# Usage (from the repository root):
#   ruche/scripts/bundle-macos.sh            # build, then bundle
#   ruche/scripts/bundle-macos.sh --no-build # bundle an existing build
#
# The build needs Xcode's Metal compiler: GPUI compiles its shaders at build
# time. The script stops with the setup commands when it is not usable.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

APP_NAME="Ruche"
BUNDLE_ID="io.github.guillaumebeal.ruche"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
APP_BUNDLE="target/$APP_NAME.app"

if [[ "${1:-}" != "--no-build" ]]; then
  # `xcrun -f metal` also finds the Xcode stub that only points at the separate
  # Metal Toolchain download, so run the compiler to know whether it is usable.
  if ! xcrun metal --version >/dev/null 2>&1; then
    cat >&2 <<'MSG'
error: no usable Metal compiler. Set up Xcode once, then rerun this script:
  sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
  sudo xcodebuild -license accept
  xcodebuild -downloadComponent MetalToolchain
MSG
    exit 1
  fi
  cargo build -p dbflux --release
fi

rm -rf "$APP_BUNDLE"
mkdir -p "$APP_BUNDLE/Contents/MacOS" "$APP_BUNDLE/Contents/Resources"

cp target/release/dbflux "$APP_BUNDLE/Contents/MacOS/dbflux"
chmod +x "$APP_BUNDLE/Contents/MacOS/dbflux"
cp resources/branding/stable/mark.svg "$APP_BUNDLE/Contents/Resources/dbflux.svg"
cp packaging/icons/dbflux.icns "$APP_BUNDLE/Contents/Resources/AppIcon.icns"
sed -e "s/\${VERSION}/$VERSION/g" -e "s/\${APP_NAME}/$APP_NAME/g" -e "s/\${BUNDLE_ID}/$BUNDLE_ID/g" \
  resources/macos/Info.plist >"$APP_BUNDLE/Contents/Info.plist"

codesign --force --deep --sign - "$APP_BUNDLE" >/dev/null

echo "Built $APP_BUNDLE ($APP_NAME $VERSION, $BUNDLE_ID)"
