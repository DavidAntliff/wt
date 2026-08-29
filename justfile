build-release:
    cargo build --release

install: build-release
    install -m 755 target/release/wt ~/.local/bin/
    install -m 644 wt-shell ~/.local/bin/
