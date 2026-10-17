#!/usr/bin/env bash
# Build Ruche and wrap it in a local macOS app bundle: target/Ruche.app.
#
# Mirrors the "Create app bundle" step of .github/workflows/build.yml with the
# Ruche identity. It is not notarised: Gatekeeper still needs a Developer ID for
# distribution.
#
# The bundle is signed with a stable code-signing identity when one exists:
# macOS ties a keychain "Always Allow" to the app's signature, and an ad-hoc
# signature changes with every build, so each rebuild would ask again for every
# saved password (and a prompt left open past DBFlux's 5 s keyring timeout fails
# the save). Set RUCHE_SIGN_IDENTITY to choose the identity; otherwise the first
# "Apple Development" or "Developer ID Application" identity is used, and ad-hoc
# signing remains the fallback.
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

SIGN_IDENTITY="${RUCHE_SIGN_IDENTITY:-}"
if [[ -z "$SIGN_IDENTITY" ]]; then
  SIGN_IDENTITY="$(security find-identity -v -p codesigning 2>/dev/null |
    grep -E '"(Apple Development|Developer ID Application):' |
    awk 'NR == 1 { print $2 }')"
fi
if [[ -z "$SIGN_IDENTITY" ]]; then
  echo "warning: no code-signing identity found; signing ad hoc, so macOS will ask again for saved passwords after each build" >&2
  SIGN_IDENTITY="-"
fi
codesign --force --deep --sign "$SIGN_IDENTITY" "$APP_BUNDLE" >/dev/null

echo "Built $APP_BUNDLE ($APP_NAME $VERSION, $BUNDLE_ID)"
