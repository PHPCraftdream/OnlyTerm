#!/bin/bash
set -x
set -e

TARGET_DIR=${1:-target}

TAG_NAME=${TAG_NAME:-$(git -c "core.abbrev=8" show -s "--format=%cd-%h" "--date=format:%Y%m%d-%H%M%S")}

echo "OSTYPE is $OSTYPE"

# OnlyTerm is Windows-only: this used to dispatch on $OSTYPE to also
# package macOS app bundles and Linux rpm/deb/apk packages, but those
# platforms are no longer supported by this fork -- see crates/window/src/os,
# which only builds the Windows backend now.
case $OSTYPE in
  msys|cygwin)
    zipdir=OnlyTerm-windows-$TAG_NAME
    if [[ "$BUILD_REASON" == "Schedule" ]] ; then
      zipname=OnlyTerm-windows-nightly.zip
      instname=OnlyTerm-nightly-setup
    else
      zipname=$zipdir.zip
      instname=OnlyTerm-${TAG_NAME}-setup
    fi
    rm -rf $zipdir $zipname
    mkdir $zipdir
    cp $TARGET_DIR/release/onlyterm.exe \
      $TARGET_DIR/release/onlyterm-mux-server.exe \
      $TARGET_DIR/release/onlyterm-gui.exe \
      $TARGET_DIR/release/strip-ansi-escapes.exe \
      $TARGET_DIR/release/onlyterm.pdb \
      $TARGET_DIR/release/onlyterm_mux_server.pdb \
      $TARGET_DIR/release/onlyterm_gui.pdb \
      assets/windows/conhost/conpty.dll \
      assets/windows/conhost/OpenConsole.exe \
      $zipdir
    7z a -tzip $zipname $zipdir
    iscc.exe -DMyAppVersion=${TAG_NAME#nightly} -F${instname} ci/windows-installer.iss
    ;;
  *)
    echo "OnlyTerm only packages the msys/cygwin (Windows) target; nothing to do for OSTYPE=$OSTYPE"
    ;;
esac
