# Maintainer: Efe Baykara <efexplose@gmail.com>
pkgname=audio-streamer
pkgver=0.1.0
pkgrel=1
pkgdesc="Stream system audio to phone via UDP"
arch=('x86_64')
url="https://github.com/efebaykaraa/audio-streamer"
license=('MIT')
depends=('ffmpeg')
makedepends=('rust' 'cargo')
source=("$pkgname-$pkgver.tar.gz::https://github.com/efebaykaraa/$pkgname/archive/v$pkgver.tar.gz")
sha256sums=('SKIP')

build() {
    cd "$pkgname-$pkgver"
    export RUSTUP_TOOLCHAIN=stable
    export CARGO_TARGET_DIR=target
    cargo build --release
}

check() {
    cd "$pkgname-$pkgver"
    export RUSTUP_TOOLCHAIN=stable
    cargo test
}

package() {
    cd "$pkgname-$pkgver"
    install -Dm0755 -t "$pkgdir/usr/bin/" "target/release/$pkgname"
    
    # Install icon
    install -Dm0644 "audio-streamer.png" "$pkgdir/usr/share/pixmaps/$pkgname.png"
    # OR for hicolor theme (recommended):
    # install -Dm0644 "audio-streamer.png" "$pkgdir/usr/share/icons/hicolor/64x64/apps/audio-streamer.png"
    
    # Install desktop file
    install -Dm0644 -t "$pkgdir/usr/share/applications/" <<EOF
[Desktop Entry]
Type=Application
Name=Audio Streamer
Comment=Stream system audio over UDP
Exec=audio-streamer
Icon=$pkgdir/usr/share/pixmaps/$pkgname.png
Categories=AudioVideo;Audio;
Terminal=false
StartupNotify=true
EOF
    
    if [ -f README.md ]; then
        install -Dm0644 README.md "$pkgdir/usr/share/doc/$pkgname/README.md"
    fi
}